//! Message send/sync domain logic, ports `services/message_service.dart`.
//!
//! Deliberately takes narrow borrowed refs (chain client, db connection,
//! keys, wallet, escrow signer) rather than `&Session` as a single
//! parameter — see the `Send`-future gotcha documented in
//! `commands/contacts.rs`: `Session` holds the Stronghold `Vault`, which
//! isn't `Sync`, so any `async fn` taking `&Session` across an `.await`
//! breaks Tauri's `Send` requirement on command futures.
//!
//! Only the blockchain sync layer is ported — mobile's `SyncLayer` enum
//! has exactly one variant (`blockchain`); the indexer-backed "Layer 2"
//! code referenced in old comments was already commented out in the Dart
//! source. `_processKeyPublications` (parsing `publishKeys` logs during
//! sync to warm the contact-key cache) is **not ported**: grepping
//! `message_service.dart`, it's defined but has zero call sites — dead
//! code, superseded by `ContactRepository.getContactKeys`'s own lazy
//! on-chain/indexer resolve (mirrored by
//! `commands::contacts::resolve_contact_keys_impl`, reused below).
//!
//! **Bug found and fixed (not reproduced) relative to the Dart source**:
//! `MessageService._processKemHandshakes` has an inverted condition —
//! `if (msgTag == null || cryptoService.constantTimeEquals(msgTag,
//! expectedTag)) continue;` skips the message precisely when the tag
//! *matches*, meaning the recipient side of the one-time PQ/KEM handshake
//! never actually decapsulates a real handshake message. The sender still
//! computes and caches its own copy of the PQ shared secret (that happens
//! in `_performKemHandshake`, untouched by this bug), so every subsequent
//! message from that sender is hybrid-encrypted with a PQ component the
//! recipient never learns — decryption then depends entirely on the
//! `decryptHybrid` classical-only fallback catch, which only works because
//! `encryptHybrid`/`decryptHybrid`'s AES-GCM step would otherwise hard-fail
//! on a key mismatch. In short: two-way post-quantum hybrid encryption is
//! silently degraded to classical-only on the receiving end. This is easy
//! to miss because PQ pubkeys are only published for a subset of accounts
//! today, so the handshake path rarely triggers. Fixed here by keeping the
//! (correct) `!matches` condition.

use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::Connection;
use thiserror::Error;

use crate::chain::client::{ChainError, SealedChainClient};
use crate::chain::escrow::TreasuryEscrowSigner;
use crate::chain::wallet::AlgorandWallet;
use crate::contacts::{self, ContactKeysUpdate};
use crate::indexer::client::IndexerClient;
use crate::keys::SealedKeys;
use crate::messages::{self, DecryptedMessage};
use crate::sync_state;

/// Fetch page size for chain/indexer message scans. Matches
/// `SealedChainClient.fetchMessages`'s Dart-side default (`limit = 200`) —
/// mobile's call sites never override it.
const FETCH_LIMIT: u32 = 200;

/// 5-minute buffer subtracted from the last-sync timestamp before an
/// incremental sync, to tolerate clock drift. Matches
/// `MessageService._syncMessages`.
const SYNC_BUFFER_SECONDS: i64 = 5 * 60;

const MEMO_VERSION: u8 = 0x02;
const PAD_ALIGNMENT: usize = 64;
/// `900 - headerSize(3)`, mirroring `MessageService._pad`'s Solana-tx-size
/// headroom comment (kept as-is; still a reasonable ceiling for Algorand).
const MAX_DATA_SIZE: usize = 900 - 3;

const KEM_DISCOVERY_TAG_KEY: &[u8] = b"sealed-kem-init-tag-v1";
const KEM_CIPHERTEXT_LEN: usize = crate::crypto::pq::PQ_CIPHERTEXT_LEN; // 768
const KEM_SCAN_PUB_LEN: usize = 32;
const KEM_HANDSHAKE_PAYLOAD_LEN: usize = KEM_CIPHERTEXT_LEN + KEM_SCAN_PUB_LEN; // 800

// ---------------------------------------------------------------------------
// Hybrid first-message frame — folds the KEM handshake + first payload into
// ONE on-chain call (1 credit) instead of the legacy handshake-then-message
// path (2 credits), for short enough first contacts. Ports the constants and
// framing from `message_codec.dart`'s "HYBRID FIRST-MESSAGE FRAME" section.
//
//   Hybrid frame:  [kemCt 768B][innerLen 2B LE][AES-GCM(inner) variable] <= 992B
//   Inner envelope (AES-256-GCM-encrypted under the raw KEM shared secret,
//   via `crypto::encrypt_with_enc_key`/`decrypt_with_enc_key`):
//                  [1B version=0x01][8B timestamp_ms BE][gzip(content)]
//
// Discrimination at receive time is length-based: `frame.len() ==
// KEM_HANDSHAKE_PAYLOAD_LEN(800)` -> legacy; `> 800` -> hybrid.
// ---------------------------------------------------------------------------

/// Hard cap on hybrid frame bytes: AVM `log` emits <= 1024B/call, and
/// `send_message` prepends a 32B sender-ephemeral-pubkey before the
/// ciphertext arg, so the budget is 1024 - 32 = 992.
const HYBRID_FRAME_MAX_BYTES: usize = 992;
const HYBRID_INNER_VERSION: u8 = 0x01;
/// `[1B version][8B timestamp_ms BE]`.
const HYBRID_INNER_HEADER_LEN: usize = 9;
/// AES-GCM overhead: 12B nonce + 16B tag (matches `crypto::aead`'s combined layout).
const AES_GCM_OVERHEAD_LEN: usize = 28;
/// Character-count pre-filter above which we skip straight to the legacy
/// 2-call send without even gzip-checking — matches
/// `kHybridFirstMessageCharThreshold`.
const HYBRID_FIRST_MESSAGE_CHAR_THRESHOLD: usize = 280;

#[derive(Debug, Error)]
pub enum MessagingError {
    #[error("no credits available. Redeem a code to send messages.")]
    NoCredits,
    #[error("recipient wallet must decode to 32 bytes")]
    InvalidRecipientWallet,
    #[error("recipient keys unavailable — could not resolve on-chain or derive a fallback")]
    RecipientKeysUnavailable,
    #[error("message too large: {0} bytes (max {MAX_DATA_SIZE})")]
    TooLarge(usize),
    #[error("chain error: {0}")]
    Chain(#[from] ChainError),
    #[error("crypto error: {0}")]
    Crypto(#[from] crate::crypto::CryptoError),
    #[error("db error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("sync state error: {0}")]
    SyncState(#[from] crate::sync_state::SyncStateError),
    #[error("contact resolve error: {0}")]
    ContactResolve(String),
    #[error("gzip error: {0}")]
    Gzip(String),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    // Not `#[from]`: `AliasError` itself has a `Messaging(#[from]
    // MessagingError)` variant, so an unboxed `Alias(AliasError)` field
    // here would make the two enums recursively contain each other by
    // value — an infinite-size type. Boxing breaks the cycle; the `From`
    // impl below is written by hand since `#[from]` only targets the
    // field's own type (`Box<AliasError>`), not `AliasError` itself.
    #[error("alias error: {0}")]
    Alias(Box<crate::alias::AliasError>),
}

/// Appends a timestamped line to `%TEMP%\sealed-desktop-sync.log` — a
/// terminal-free diagnostic trail for background-sync failures (alias
/// handshake dispatch errors, in particular). Deliberately not routed
/// through `app_dir`/`Session` (would need threading a new parameter
/// through several call sites just for this): `std::env::temp_dir()` is
/// always reachable with zero plumbing, and this is a debugging aid, not
/// something the app depends on functioning.
pub(crate) fn log_sync_diagnostic(msg: &str) {
    use std::io::Write;
    let path = std::env::temp_dir().join("sealed-desktop-sync.log");
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "[{}] {}", now_unix_millis(), msg);
    }
}

/// First 8 bytes of `bytes`, hex-encoded — a short, log-friendly
/// fingerprint for comparing whether two secret byte strings are the same
/// value across two points in the code (or, eventually, across two
/// devices) without ever writing the full secret to disk. Not a
/// cryptographic hash (doesn't need to be — this never leaves the local
/// diagnostic log, and a truncated prefix is already astronomically
/// unlikely to collide between genuinely different 32-byte secrets).
pub(crate) fn hex_fingerprint(bytes: &[u8]) -> String {
    bytes.iter().take(8).map(|b| format!("{b:02x}")).collect()
}

impl From<crate::alias::AliasError> for MessagingError {
    fn from(e: crate::alias::AliasError) -> Self {
        MessagingError::Alias(Box::new(e))
    }
}

fn now_unix_millis() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Padding / framing helpers — mirror `_pad`/`_unpad`/`_combineCiphertexts`/
// `_splitCiphertexts` in message_service.dart.
// ---------------------------------------------------------------------------

/// `[1-byte version][2-byte LE length][data][zero padding to a 64-byte
/// boundary]`. `pub(crate)`: also used by `alias::messaging` for the
/// alias-chat wire format, which reuses this exact outer-padding layer.
pub(crate) fn pad_message(data: &[u8]) -> Result<Vec<u8>, MessagingError> {
    const HEADER_SIZE: usize = 3;
    if data.len() > MAX_DATA_SIZE {
        return Err(MessagingError::TooLarge(data.len()));
    }
    let total_unpadded = HEADER_SIZE + data.len();
    let padded_size = total_unpadded.div_ceil(PAD_ALIGNMENT) * PAD_ALIGNMENT;

    let mut padded = vec![0u8; padded_size];
    padded[0] = MEMO_VERSION;
    padded[1] = (data.len() & 0xFF) as u8;
    padded[2] = ((data.len() >> 8) & 0xFF) as u8;
    padded[HEADER_SIZE..HEADER_SIZE + data.len()].copy_from_slice(data);
    Ok(padded)
}

pub(crate) fn unpad_message(data: &[u8]) -> Option<Vec<u8>> {
    if data.is_empty() || data[0] != MEMO_VERSION || data.len() < 3 {
        return None;
    }
    let original_len = (data[1] as usize) | ((data[2] as usize) << 8);
    if original_len > data.len() - 3 {
        return None;
    }
    Some(data[3..3 + original_len].to_vec())
}

/// `[2-byte LE recipient_ct_len][recipient_ct][self_ct]`. `pub(crate)`:
/// also used by `alias::messaging` — mobile's alias send path
/// (`MessageSender._send`/`_sendRawBytes`) always wraps via
/// `combineCiphertexts(ciphertext, emptyBytes)` even for alias sends with
/// no real self-copy, so this module's own alias code must match that
/// framing exactly (see [`split_ciphertexts`]'s doc comment for the
/// corresponding bug this was found alongside).
pub(crate) fn combine_ciphertexts(recipient_ct: &[u8], self_ct: &[u8]) -> Vec<u8> {
    let mut combined = Vec::with_capacity(2 + recipient_ct.len() + self_ct.len());
    combined.push((recipient_ct.len() & 0xFF) as u8);
    combined.push(((recipient_ct.len() >> 8) & 0xFF) as u8);
    combined.extend_from_slice(recipient_ct);
    combined.extend_from_slice(self_ct);
    combined
}

/// **Bug found and fixed 2026-08-12**: `alias::messaging::send_alias_message_network`/
/// `apply_alias_sync_result` used to skip this framing entirely — sending a
/// bare `pad_message(ciphertext)` and receiving via `unpad_message` alone,
/// no split. But mobile's actual alias-send code (confirmed by reading
/// `message_sender.dart`'s `_send`/`_sendRawBytes`, both of which
/// `sendAliasMessage`/the invite/accept envelope path route through) always
/// wraps via `combineCiphertexts(ciphertext, Uint8List(0))` before padding,
/// exactly like regular DMs (just with an empty self-copy instead of a real
/// one) — there is no bare/unwrapped ciphertext wire shape anywhere in the
/// live protocol. Receiving a combine-wrapped ciphertext as if it were bare
/// prepends 2 stray length-prefix bytes onto what `decrypt_hybrid` sees,
/// which reliably fails the AES-GCM auth-tag check — this was the actual
/// cause of a live "alias handshake works, alias *messages* never decrypt
/// in either direction" report that looked like a PQ key mismatch (tag
/// matching uses a different keypair than decryption, so it kept
/// succeeding and pointing the investigation the wrong way).
pub(crate) fn split_ciphertexts(combined: &[u8]) -> Option<(Vec<u8>, Vec<u8>)> {
    if combined.len() < 4 {
        return None;
    }
    let recipient_len = (combined[0] as usize) | ((combined[1] as usize) << 8);
    if 2 + recipient_len > combined.len() {
        return None;
    }
    let recipient_ct = combined[2..2 + recipient_len].to_vec();
    let self_ct = combined[2 + recipient_len..].to_vec();
    Some((recipient_ct, self_ct))
}

pub(crate) fn gzip_compress(data: &[u8]) -> Result<Vec<u8>, MessagingError> {
    use std::io::Write;
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(data).map_err(|e| MessagingError::Gzip(e.to_string()))?;
    encoder.finish().map_err(|e| MessagingError::Gzip(e.to_string()))
}

pub(crate) fn gzip_decompress(data: &[u8]) -> Result<Vec<u8>, MessagingError> {
    use std::io::Read;
    let mut decoder = flate2::read::GzDecoder::new(data);
    let mut out = Vec::new();
    decoder.read_to_end(&mut out).map_err(|e| MessagingError::Gzip(e.to_string()))?;
    Ok(out)
}

// ---------------------------------------------------------------------------
// Hybrid first-message frame codec — ports `encodeHybridFirstFrame` /
// `decodeHybridFirstFrame` / `encodeHybridInnerEnvelope` /
// `decodeHybridInnerEnvelope` / `hybridFrameFits` from `message_codec.dart`.
// Unlike the Dart source (which throws typed exceptions the caller catches),
// these return `Option` — every failure mode here (frame too small/large,
// malformed length prefix, wrong version byte) converges on the same
// "treat as absent / fall back" behavior at both call sites, so a richer
// error type would add ceremony without adding information.
// ---------------------------------------------------------------------------

/// True iff a hybrid frame carrying `gzipped_content_len` bytes of gzipped
/// content would fit under [`HYBRID_FRAME_MAX_BYTES`] after envelope +
/// AES-GCM + framing overhead. Formula matches [`encode_hybrid_first_frame`]
/// exactly: predicate-true implies the encode below always succeeds.
fn hybrid_frame_fits(gzipped_content_len: usize) -> bool {
    let frame_len = KEM_CIPHERTEXT_LEN + 2 + HYBRID_INNER_HEADER_LEN + gzipped_content_len + AES_GCM_OVERHEAD_LEN;
    frame_len <= HYBRID_FRAME_MAX_BYTES
}

/// `[1B version][8B timestamp_ms BE][gzip(content)]` — the plaintext later
/// AES-256-GCM-encrypted (via `crypto::encrypt_with_enc_key`) under the raw
/// KEM shared secret. Note this carries the raw message `content` only, not
/// the full `MessagePayload` JSON envelope regular messages use — there's no
/// room, and the recipient already learns the sender from the on-chain txn's
/// sender field plus the deterministic discovery tag.
fn encode_hybrid_inner_envelope(timestamp_ms: i64, content: &[u8]) -> Result<Vec<u8>, MessagingError> {
    let gz = gzip_compress(content)?;
    let mut out = Vec::with_capacity(HYBRID_INNER_HEADER_LEN + gz.len());
    out.push(HYBRID_INNER_VERSION);
    out.extend_from_slice(&(timestamp_ms as u64).to_be_bytes());
    out.extend_from_slice(&gz);
    Ok(out)
}

/// Inverse of [`encode_hybrid_inner_envelope`]. `None` on an unknown version
/// byte, a too-short buffer, or a gzip decode failure.
fn decode_hybrid_inner_envelope(bytes: &[u8]) -> Option<(i64, Vec<u8>)> {
    if bytes.len() < HYBRID_INNER_HEADER_LEN || bytes[0] != HYBRID_INNER_VERSION {
        return None;
    }
    let timestamp_ms = u64::from_be_bytes(bytes[1..9].try_into().ok()?) as i64;
    let content = gzip_decompress(&bytes[HYBRID_INNER_HEADER_LEN..]).ok()?;
    Some((timestamp_ms, content))
}

/// `[kemCt 768B][innerLen 2B LE][innerCiphertext]`. `None` if
/// `inner_ciphertext` doesn't fit a 2-byte length prefix, or the total would
/// collide with the legacy [`KEM_HANDSHAKE_PAYLOAD_LEN`]-byte handshake
/// frame (which would make receivers misparse it as legacy) or exceed
/// [`HYBRID_FRAME_MAX_BYTES`]. Callers should check [`hybrid_frame_fits`]
/// first so this is expected to always succeed in practice.
fn encode_hybrid_first_frame(kem_ct: &[u8; KEM_CIPHERTEXT_LEN], inner_ciphertext: &[u8]) -> Option<Vec<u8>> {
    if inner_ciphertext.len() > 0xFFFF {
        return None;
    }
    let total = KEM_CIPHERTEXT_LEN + 2 + inner_ciphertext.len();
    if total <= KEM_HANDSHAKE_PAYLOAD_LEN || total > HYBRID_FRAME_MAX_BYTES {
        return None;
    }
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(kem_ct);
    out.push((inner_ciphertext.len() & 0xFF) as u8);
    out.push(((inner_ciphertext.len() >> 8) & 0xFF) as u8);
    out.extend_from_slice(inner_ciphertext);
    Some(out)
}

/// `None` when `frame` is too small to be hybrid (caller should try the
/// legacy [`KEM_HANDSHAKE_PAYLOAD_LEN`]-byte parse instead) or too large, or
/// structurally malformed (bad length prefix) — all three cases mean "drop
/// this candidate", matching `processKemHandshakes`' handling of the
/// equivalent `null` / caught-`FormatException` outcomes.
fn decode_hybrid_first_frame(frame: &[u8]) -> Option<([u8; KEM_CIPHERTEXT_LEN], Vec<u8>)> {
    if frame.len() <= KEM_HANDSHAKE_PAYLOAD_LEN || frame.len() > HYBRID_FRAME_MAX_BYTES {
        return None;
    }
    if frame.len() < KEM_CIPHERTEXT_LEN + 2 {
        return None;
    }
    let mut kem_ct = [0u8; KEM_CIPHERTEXT_LEN];
    kem_ct.copy_from_slice(&frame[..KEM_CIPHERTEXT_LEN]);
    let inner_len = (frame[KEM_CIPHERTEXT_LEN] as usize) | ((frame[KEM_CIPHERTEXT_LEN + 1] as usize) << 8);
    if KEM_CIPHERTEXT_LEN + 2 + inner_len != frame.len() {
        return None;
    }
    Some((kem_ct, frame[KEM_CIPHERTEXT_LEN + 2..].to_vec()))
}

/// `HMAC-SHA256(key = "sealed-kem-init-tag-v1", msg = senderWallet ||
/// recipientWallet)` — note the key/message roles are the label and the
/// wallet-pair respectively, the reverse of `recipient_tag_from_secret`'s
/// (key=sharedSecret, msg=label). Matches
/// `MessageService._computeKemDiscoveryTag` exactly.
fn compute_kem_discovery_tag(sender_wallet: &str, recipient_wallet: &str) -> [u8; 32] {
    let mut input = Vec::with_capacity(sender_wallet.len() + recipient_wallet.len());
    input.extend_from_slice(sender_wallet.as_bytes());
    input.extend_from_slice(recipient_wallet.as_bytes());
    crate::crypto::kdf::hmac_sha256(KEM_DISCOVERY_TAG_KEY, &input)
}

/// `pub(crate)`: also built/parsed by `alias::messaging` — mobile's
/// `sendAliasMessage` routes through the exact same JSON-envelope `_send()`
/// helper regular DMs use (confirmed by reading `message_sender.dart`),
/// just with `recipient_wallet` set to the alias `contactId` (not a real
/// wallet address) and `recipient_username` set to the alias contact's
/// label — there is no separate, simpler wire format for alias messages.
#[derive(serde::Serialize, serde::Deserialize)]
pub(crate) struct MessagePayload {
    pub(crate) sender_wallet: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) sender_username: Option<String>,
    pub(crate) recipient_wallet: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) recipient_username: Option<String>,
    pub(crate) content: String,
    pub(crate) timestamp: i64,
}

// ---------------------------------------------------------------------------
// Send
// ---------------------------------------------------------------------------

pub struct SendMessageRequest<'a> {
    pub sender_wallet: &'a str,
    pub sender_username: Option<&'a str>,
    pub recipient_wallet: &'a str,
    pub recipient_username: Option<&'a str>,
    pub plaintext: &'a str,
}

/// Everything `send_message_network` produces that still needs to be
/// written to the database — kept as owned data specifically so that
/// writing it (via [`apply_send_result`]) never has to happen inside the
/// same `async fn` as a `.await`. `rusqlite::Connection` (and everything
/// that embeds it — `Db`, `Session`) holds a `RefCell`-based prepared
/// statement cache internally, which makes `!Sync`, and therefore `&Db`/
/// `&Connection` isn't `Send`. Any local variable of one of those types
/// that's read both before *and* after the same `.await` — even with
/// nothing but unrelated work in between — makes the whole `async fn`'s
/// generated future non-`Send`, which `#[tauri::command]` requires. So
/// every async/network step in this module is split from every sync/db
/// step into separate functions, joined only by owned data.
pub struct SendMessageOutcome {
    pub tx_id: String,
    pub message: DecryptedMessage,
    pub contact_keys_update: ContactKeysUpdate,
}

/// Network/crypto half of sending a message: credits pre-flight, resolve
/// recipient keys (starting from an already-cached lookup the caller
/// supplies, falling back to the same lazy on-chain/indexer resolve
/// `commands::contacts::resolve_contact_keys` exposes), one-time PQ/KEM
/// handshake if needed, hybrid-encrypt for both the recipient and a
/// self-copy, and submit on-chain. Returns everything [`apply_send_result`]
/// needs to persist afterward. Mirrors `MessageService.sendMessage`, minus
/// its local-cache writes (see [`apply_send_result`]).
#[allow(clippy::too_many_arguments)]
pub async fn send_message_network(
    chain_client: &SealedChainClient,
    indexer_client: &IndexerClient,
    wallet: &AlgorandWallet,
    escrow: &TreasuryEscrowSigner,
    sealed_keys: &SealedKeys,
    request: SendMessageRequest<'_>,
    cached: crate::contacts::ContactKeys,
) -> Result<SendMessageOutcome, MessagingError> {
    let credits = chain_client.get_credits(wallet, &wallet.address).await?;
    if credits < 1 {
        return Err(MessagingError::NoCredits);
    }

    if crate::chain::address::decode_address(request.recipient_wallet).is_none() {
        return Err(MessagingError::InvalidRecipientWallet);
    }

    let resolved = crate::commands::contacts::resolve_contact_keys_impl(chain_client, indexer_client, request.recipient_wallet, cached)
        .await
        .map_err(MessagingError::ContactResolve)?;

    let recipient_encryption_pubkey = resolved.encryption_pubkey.ok_or(MessagingError::RecipientKeysUnavailable)?;
    let recipient_scan_pubkey = resolved.scan_pubkey.ok_or(MessagingError::RecipientKeysUnavailable)?;
    let recipient_pq_pubkey = resolved.pq_public_key.clone();
    let mut pq_shared_secret = resolved.pq_shared_secret.clone();

    // One fresh X25519 keypair for this message, reused for three ECDH
    // computations (tag, recipient-encrypt, self-copy-encrypt) — see
    // `crypto::x25519::ReusableKeyPair`'s doc comment for why this can't
    // be the single-use `EphemeralKeyPair`.
    let ephemeral = crate::crypto::x25519::generate_reusable_keypair();

    let shared_for_tag = ephemeral.secret.diffie_hellman(&crate::crypto::x25519::public_key_from_bytes(&recipient_scan_pubkey));
    let recipient_tag = crate::crypto::recipient_tag_from_secret(shared_for_tag.as_bytes());

    let timestamp_ms = now_unix_millis();

    // One-time PQ/KEM handshake per contact — only if the recipient has
    // published a PQ pubkey and we don't already have a shared secret
    // cached for them.
    if let Some(pq_pub) = &recipient_pq_pubkey {
        if pq_shared_secret.is_none() {
            let kem = crate::crypto::pq::kem_encapsulate(pq_pub)?;
            let kem_tag = compute_kem_discovery_tag(request.sender_wallet, request.recipient_wallet);

            // Try to fold the handshake + this first message into ONE
            // on-chain call (1 credit) instead of the legacy handshake-then-
            // message path below (2 credits) — matches
            // `MessageSender.sendMessage`'s branch A/B/C selection. Only
            // attempted for short-enough plaintext; the char-count check is
            // a cheap pre-filter before the byte-precise `hybrid_frame_fits`.
            let plaintext_bytes = request.plaintext.as_bytes();
            let hybrid_eligible = request.plaintext.chars().count() <= HYBRID_FIRST_MESSAGE_CHAR_THRESHOLD
                && hybrid_frame_fits(gzip_compress(plaintext_bytes)?.len());

            let hybrid_frame = if hybrid_eligible {
                let envelope = encode_hybrid_inner_envelope(timestamp_ms, plaintext_bytes)?;
                let inner_ciphertext = crate::crypto::encrypt_with_enc_key(&kem.shared_secret, &envelope)?;
                encode_hybrid_first_frame(&kem.ciphertext, &inner_ciphertext)
            } else {
                None
            };

            if let Some(frame) = hybrid_frame {
                // The outer 32-byte prefix `send_message` embeds is unused
                // on the receive side for KEM-tagged frames (scanning is
                // driven by the deterministic discovery tag, not this
                // slot) — any 32 bytes do, matching the legacy handshake
                // send below.
                let filler = crate::crypto::x25519::generate_ephemeral_keypair();
                let tx_id = chain_client.send_message(wallet, escrow, &kem_tag, &filler.public, &frame).await?;
                return Ok(SendMessageOutcome {
                    tx_id: tx_id.clone(),
                    message: DecryptedMessage {
                        id: tx_id.clone(),
                        sender_wallet: request.sender_wallet.to_string(),
                        sender_username: request.sender_username.map(str::to_string),
                        recipient_wallet: request.recipient_wallet.to_string(),
                        recipient_username: request.recipient_username.map(str::to_string),
                        content: request.plaintext.to_string(),
                        timestamp: timestamp_ms / 1000,
                        is_outgoing: true,
                        on_chain_pubkey: tx_id,
                    },
                    contact_keys_update: ContactKeysUpdate {
                        encryption_pubkey: Some(recipient_encryption_pubkey),
                        scan_pubkey: Some(recipient_scan_pubkey),
                        pq_public_key: recipient_pq_pubkey,
                        pq_shared_secret: Some(kem.shared_secret.to_vec()),
                    },
                });
            }

            // Not hybrid-eligible (or the frame didn't fit) — legacy 2-call
            // path: send the handshake now, fall through below to send the
            // regular (JSON-enveloped, ECDH-encrypted) message.
            pq_shared_secret = Some(kem.shared_secret.to_vec());
            let mut kem_payload = kem.ciphertext.to_vec();
            kem_payload.extend_from_slice(&sealed_keys.scan_pubkey);
            let filler = crate::crypto::x25519::generate_ephemeral_keypair();
            chain_client.send_message(wallet, escrow, &kem_tag, &filler.public, &kem_payload).await?;
        }
    }

    let payload = MessagePayload {
        sender_wallet: request.sender_wallet.to_string(),
        sender_username: request.sender_username.map(str::to_string),
        recipient_wallet: request.recipient_wallet.to_string(),
        recipient_username: request.recipient_username.map(str::to_string),
        content: request.plaintext.to_string(),
        timestamp: timestamp_ms,
    };
    let payload_bytes = serde_json::to_vec(&payload)?;
    let compressed = gzip_compress(&payload_bytes)?;

    let shared_for_recipient =
        ephemeral.secret.diffie_hellman(&crate::crypto::x25519::public_key_from_bytes(&recipient_encryption_pubkey));
    let recipient_ciphertext = crate::crypto::encrypt_hybrid(&compressed, shared_for_recipient.as_bytes(), pq_shared_secret.as_deref())?;

    // Self-copy so we can decrypt our own sent messages later — classical
    // only, matching the Dart source.
    let shared_for_self = ephemeral.secret.diffie_hellman(&crate::crypto::x25519::public_key_from_bytes(&sealed_keys.encryption_pubkey));
    let self_ciphertext = crate::crypto::encrypt_hybrid(&compressed, shared_for_self.as_bytes(), None)?;

    let combined = combine_ciphertexts(&recipient_ciphertext, &self_ciphertext);
    let padded = pad_message(&combined)?;

    let tx_id = chain_client.send_message(wallet, escrow, &recipient_tag, &ephemeral.public, &padded).await?;

    Ok(SendMessageOutcome {
        tx_id: tx_id.clone(),
        message: DecryptedMessage {
            id: tx_id.clone(),
            sender_wallet: request.sender_wallet.to_string(),
            sender_username: request.sender_username.map(str::to_string),
            recipient_wallet: request.recipient_wallet.to_string(),
            recipient_username: request.recipient_username.map(str::to_string),
            content: request.plaintext.to_string(),
            timestamp: timestamp_ms / 1000,
            is_outgoing: true,
            on_chain_pubkey: tx_id,
        },
        contact_keys_update: ContactKeysUpdate {
            encryption_pubkey: Some(recipient_encryption_pubkey),
            scan_pubkey: Some(recipient_scan_pubkey),
            pq_public_key: recipient_pq_pubkey,
            pq_shared_secret,
        },
    })
}

/// Everything [`send_raw_bytes_network`] produces that still needs to be
/// written to the database — same reasoning as [`SendMessageOutcome`]'s doc
/// comment. No `message`/`DecryptedMessage` field: raw-bytes sends (alias
/// invite/accept envelopes) never appear as a chat bubble, so there's
/// nothing to save to the `messages` table.
pub struct SendRawBytesOutcome {
    // Both call sites in `commands::alias.rs` return their own DTO instead
    // of the tx id — unlike `send_message`'s outcome, nothing surfaces this
    // to the frontend today.
    #[allow(dead_code)]
    pub tx_id: String,
    pub contact_keys_update: ContactKeysUpdate,
}

/// Delivers `payload` directly as a regular DM to `recipient_wallet`,
/// skipping the JSON+gzip envelope and self-copy that
/// [`send_message_network`] always applies — used exclusively to deliver
/// alias-chat invite/accept envelopes (`alias::envelope::{InviteEnvelope,
/// AcceptEnvelope}`, Phase 7h) as an ordinary message to an already-known
/// wallet contact, instead of only via QR/paste. Never used for
/// user-authored text.
///
/// No hybrid-first-message-frame attempt: an 865-byte invite envelope can
/// never fit the 992-byte hybrid budget alongside a 768-byte KEM
/// ciphertext, so the eligibility check `send_message_network` runs would
/// always be false here — skipped rather than computed and discarded.
///
/// Always wraps via `combine_ciphertexts(recipient_ct, &[])` (empty
/// self-copy) rather than sending `recipient_ct` bare: `sync_incoming_messages`'s
/// `split_ciphertexts` always reads the padded payload's first 2 bytes as a
/// length prefix, and AES-GCM ciphertext is effectively random — a bare
/// ciphertext has a real (~1.4%) chance its first two bytes look like a
/// plausible length, causing a misparse. The explicit empty-self-copy
/// wrapper makes the split always land correctly, with zero receive-side
/// changes needed. Size budget (invite envelope, the tighter of the two):
/// 865B payload -> 893B recipient_ct (+28B AES-GCM overhead) -> 895B
/// combined (+2B length prefix) <= `MAX_DATA_SIZE` (897B) — only 2 bytes of
/// headroom, guarded by a unit test below.
#[allow(clippy::too_many_arguments)]
pub async fn send_raw_bytes_network(
    chain_client: &SealedChainClient,
    indexer_client: &IndexerClient,
    wallet: &AlgorandWallet,
    escrow: &TreasuryEscrowSigner,
    sealed_keys: &SealedKeys,
    recipient_wallet: &str,
    payload: &[u8],
    cached: crate::contacts::ContactKeys,
) -> Result<SendRawBytesOutcome, MessagingError> {
    let credits = chain_client.get_credits(wallet, &wallet.address).await?;
    if credits < 1 {
        return Err(MessagingError::NoCredits);
    }

    if crate::chain::address::decode_address(recipient_wallet).is_none() {
        return Err(MessagingError::InvalidRecipientWallet);
    }

    let resolved = crate::commands::contacts::resolve_contact_keys_impl(chain_client, indexer_client, recipient_wallet, cached)
        .await
        .map_err(MessagingError::ContactResolve)?;

    let recipient_encryption_pubkey = resolved.encryption_pubkey.ok_or(MessagingError::RecipientKeysUnavailable)?;
    let recipient_scan_pubkey = resolved.scan_pubkey.ok_or(MessagingError::RecipientKeysUnavailable)?;
    let recipient_pq_pubkey = resolved.pq_public_key.clone();
    let mut pq_shared_secret = resolved.pq_shared_secret.clone();

    // One fresh X25519 keypair for this message, reused for both ECDH
    // computations (tag, recipient-encrypt) — see `send_message_network`'s
    // matching comment.
    let ephemeral = crate::crypto::x25519::generate_reusable_keypair();
    let shared_for_tag = ephemeral.secret.diffie_hellman(&crate::crypto::x25519::public_key_from_bytes(&recipient_scan_pubkey));
    let recipient_tag = crate::crypto::recipient_tag_from_secret(shared_for_tag.as_bytes());

    // One-time PQ/KEM handshake per contact, legacy path only (see doc
    // comment above for why the hybrid-frame branch is skipped entirely).
    if let Some(pq_pub) = &recipient_pq_pubkey {
        if pq_shared_secret.is_none() {
            let kem = crate::crypto::pq::kem_encapsulate(pq_pub)?;
            let kem_tag = compute_kem_discovery_tag(&wallet.address, recipient_wallet);
            pq_shared_secret = Some(kem.shared_secret.to_vec());
            let mut kem_payload = kem.ciphertext.to_vec();
            kem_payload.extend_from_slice(&sealed_keys.scan_pubkey);
            let filler = crate::crypto::x25519::generate_ephemeral_keypair();
            chain_client.send_message(wallet, escrow, &kem_tag, &filler.public, &kem_payload).await?;
        }
    }

    let shared_for_recipient =
        ephemeral.secret.diffie_hellman(&crate::crypto::x25519::public_key_from_bytes(&recipient_encryption_pubkey));
    let recipient_ciphertext = crate::crypto::encrypt_hybrid(payload, shared_for_recipient.as_bytes(), pq_shared_secret.as_deref())?;

    let combined = combine_ciphertexts(&recipient_ciphertext, &[]);
    let padded = pad_message(&combined)?;

    let tx_id = chain_client.send_message(wallet, escrow, &recipient_tag, &ephemeral.public, &padded).await?;

    Ok(SendRawBytesOutcome {
        tx_id,
        contact_keys_update: ContactKeysUpdate {
            encryption_pubkey: Some(recipient_encryption_pubkey),
            scan_pubkey: Some(recipient_scan_pubkey),
            pq_public_key: recipient_pq_pubkey,
            pq_shared_secret,
        },
    })
}

/// Sync half of sending a message — writes what
/// [`send_message_network`] resolved. Purely synchronous (no `.await`
/// anywhere in this function or its callees), so it's safe to call with a
/// live `&Connection` regardless of what happened before or will happen
/// after in the caller.
pub fn apply_send_result(conn: &Connection, recipient_wallet: &str, outcome: &SendMessageOutcome) -> Result<(), MessagingError> {
    messages::save_message(conn, &outcome.message)?;
    contacts::save_contact_keys(conn, recipient_wallet, &outcome.contact_keys_update)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Sync
// ---------------------------------------------------------------------------

/// Chain messages fetched for one sync pass — three separate scans,
/// matching the Dart source doing the same (`_processKemHandshakes` and
/// `_syncIncomingMessages` each independently call `fetchMessages` with
/// the same params; `_syncOutgoingMessages` calls
/// `fetchMessagesFromSender`). Kept as owned data so the network fetch
/// (this struct's construction, in [`fetch_sync_data`]) and the db writes
/// that follow (in [`apply_sync_result`]) never share an `async fn` — see
/// [`SendMessageOutcome`]'s doc comment for why.
pub struct SyncFetchResult {
    kem_candidates: Vec<crate::chain::client::ChainMessage>,
    incoming_candidates: Vec<crate::chain::client::ChainMessage>,
    outgoing_candidates: Vec<crate::chain::client::ChainMessage>,
    /// Sender wallet -> on-chain username, resolved for exactly the KEM
    /// candidates that look like an addressed-to-us hybrid first-message
    /// frame (length > legacy handshake length, discovery tag matches).
    /// Hybrid frames carry no embedded username (no room — that's the whole
    /// point of the size optimization), so showing something other than a
    /// raw wallet address for a brand-new contact's first message needs a
    /// live lookup, same as `processKemHandshakes`' per-run
    /// `chainUsernameBySender` cache.
    hybrid_sender_usernames: std::collections::HashMap<String, Option<String>>,
}

impl SyncFetchResult {
    /// Exposes the already-fetched incoming-message candidates so
    /// `alias::messaging::apply_alias_sync_result` can trial-match them
    /// against alias contacts without a second indexer round-trip. The
    /// other two fields stay private — nothing outside this module needs
    /// them.
    pub fn incoming_candidates(&self) -> &[crate::chain::client::ChainMessage] {
        &self.incoming_candidates
    }
}

/// Sync-only half: decide the `sinceTimestamp` cutoff. `full_sync` forces
/// from-genesis (`Some(0)`); otherwise the last-sync time minus a 5-minute
/// clock-drift buffer, matching `MessageService._syncMessages`.
pub fn compute_since_timestamp(conn: &Connection, full_sync: bool) -> Result<Option<i64>, MessagingError> {
    if full_sync {
        return Ok(Some(0));
    }
    let last_sync_millis = sync_state::last_sync_time_millis(conn)?;
    let buffered_millis = last_sync_millis - SYNC_BUFFER_SECONDS * 1000;
    Ok(Some(buffered_millis / 1000))
}

/// Async-only half: the three chain scans a sync pass needs. No `conn`
/// parameter at all, so it's trivially safe to `.await` from a caller that
/// also touches the database on either side.
pub async fn fetch_sync_data(
    chain_client: &SealedChainClient,
    wallet: &AlgorandWallet,
    since_timestamp: Option<i64>,
) -> Result<SyncFetchResult, MessagingError> {
    let kem_candidates = chain_client.fetch_messages(since_timestamp, FETCH_LIMIT).await?;
    let incoming_candidates = chain_client.fetch_messages(since_timestamp, FETCH_LIMIT).await?;
    let outgoing_candidates = chain_client.fetch_messages_from_sender(&wallet.address, since_timestamp, FETCH_LIMIT).await?;

    let mut hybrid_sender_usernames = std::collections::HashMap::new();
    for msg in &kem_candidates {
        if msg.sender_address.is_empty() || msg.ciphertext.len() <= KEM_HANDSHAKE_PAYLOAD_LEN {
            continue; // legacy-length, empty sender, or malformed — no live lookup needed
        }
        if hybrid_sender_usernames.contains_key(&msg.sender_address) {
            continue;
        }
        let expected_tag = compute_kem_discovery_tag(&msg.sender_address, &wallet.address);
        if !crate::crypto::constant_time_equals(&msg.recipient_tag, &expected_tag) {
            continue;
        }
        let username = chain_client.get_user_by_wallet(&msg.sender_address).await.ok().flatten().and_then(|p| p.username);
        hybrid_sender_usernames.insert(msg.sender_address.clone(), username);
    }

    Ok(SyncFetchResult { kem_candidates, incoming_candidates, outgoing_candidates, hybrid_sender_usernames })
}

/// Sync-only half: KEM handshake processing, then incoming, then outgoing
/// messages, over already-fetched chain data — no `.await` anywhere in
/// this function or its callees. Returns the number of newly-cached
/// messages. Mirrors `MessageService._syncMessages`/`_syncViaBlockchain`
/// (the overlapping-sync guard, `_activeSync`, is Dart-side call-site
/// concurrency control specific to a long-lived service object; Tauri
/// commands are invoked one at a time from the frontend, so it isn't
/// reproduced here).
pub fn apply_sync_result(conn: &mut Connection, wallet: &AlgorandWallet, sealed_keys: &SealedKeys, fetch: &SyncFetchResult) -> Result<i64, MessagingError> {
    let hybrid = process_kem_handshakes(conn, wallet, sealed_keys, &fetch.kem_candidates, &fetch.hybrid_sender_usernames)?;
    let incoming = sync_incoming_messages(conn, wallet, sealed_keys, &fetch.incoming_candidates)?;
    let outgoing = sync_outgoing_messages(conn, wallet, sealed_keys, &fetch.outgoing_candidates)?;

    sync_state::update_last_sync_time_millis(conn, now_unix_millis())?;
    Ok(hybrid + incoming + outgoing)
}

/// Handles both KEM frame shapes: legacy (`== KEM_HANDSHAKE_PAYLOAD_LEN`,
/// caches the secret only) and hybrid (`> KEM_HANDSHAKE_PAYLOAD_LEN`,
/// decrypts + saves the first message too). Returns the number of hybrid
/// messages newly saved (legacy handshakes never carry a message).
fn process_kem_handshakes(
    conn: &Connection,
    wallet: &AlgorandWallet,
    sealed_keys: &SealedKeys,
    candidates: &[crate::chain::client::ChainMessage],
    hybrid_sender_usernames: &std::collections::HashMap<String, Option<String>>,
) -> Result<i64, MessagingError> {
    let my_address = &wallet.address;
    let mut hybrid_saved = 0i64;

    for msg in candidates {
        if msg.sender_address.is_empty() {
            continue;
        }
        let existing = contacts::get_contact_keys(conn, &msg.sender_address)?;
        if existing.pq_shared_secret.is_some() {
            continue;
        }

        let expected_tag = compute_kem_discovery_tag(&msg.sender_address, my_address);
        // See the module doc comment: the Dart source inverts this check
        // (skips *matching* messages). This is the fixed, correct version.
        if !crate::crypto::constant_time_equals(&msg.recipient_tag, &expected_tag) {
            continue;
        }

        if msg.ciphertext.len() == KEM_HANDSHAKE_PAYLOAD_LEN {
            // Legacy frame: cache the secret, no message payload to save.
            let kem_ciphertext = &msg.ciphertext[..KEM_CIPHERTEXT_LEN];
            if let Ok(shared_secret) = crate::crypto::pq::kem_decapsulate(kem_ciphertext, &sealed_keys.pq_private_key) {
                contacts::save_contact_keys(
                    conn,
                    &msg.sender_address,
                    &ContactKeysUpdate { pq_shared_secret: Some(shared_secret.to_vec()), ..Default::default() },
                )?;
            }
            continue;
        }

        let Some((kem_ciphertext, inner_ciphertext)) = decode_hybrid_first_frame(&msg.ciphertext) else {
            continue; // too small/large, or malformed — not a KEM frame we understand
        };
        let Ok(shared_secret) = crate::crypto::pq::kem_decapsulate(&kem_ciphertext, &sealed_keys.pq_private_key) else {
            continue;
        };
        // Authenticate the trailing payload before trusting the secret at
        // all — an AES-GCM auth failure means the frame is tampered or the
        // secret doesn't match; don't cache, don't save, drop it entirely.
        let Ok(envelope_bytes) = crate::crypto::decrypt_with_enc_key(&shared_secret, &inner_ciphertext) else {
            continue;
        };
        contacts::save_contact_keys(
            conn,
            &msg.sender_address,
            &ContactKeysUpdate { pq_shared_secret: Some(shared_secret.to_vec()), ..Default::default() },
        )?;

        let Some((timestamp_ms, content)) = decode_hybrid_inner_envelope(&envelope_bytes) else {
            continue; // secret cached above; payload just couldn't be parsed
        };
        if messages::has_message(conn, &msg.account_pubkey)? {
            continue;
        }
        let sender_username = hybrid_sender_usernames.get(&msg.sender_address).cloned().flatten();
        messages::save_message(
            conn,
            &DecryptedMessage {
                id: msg.account_pubkey.clone(),
                sender_wallet: msg.sender_address.clone(),
                sender_username,
                recipient_wallet: my_address.clone(),
                recipient_username: None,
                content: String::from_utf8_lossy(&content).into_owned(),
                timestamp: timestamp_ms / 1000,
                is_outgoing: false,
                on_chain_pubkey: msg.account_pubkey.clone(),
            },
        )?;
        hybrid_saved += 1;
    }

    Ok(hybrid_saved)
}

fn sync_incoming_messages(
    conn: &mut Connection,
    wallet: &AlgorandWallet,
    sealed_keys: &SealedKeys,
    candidates: &[crate::chain::client::ChainMessage],
) -> Result<i64, MessagingError> {
    // Wallet-derived X25519 fallback keypair — for messages sent by
    // clients that used the Ed25519->X25519 conversion of our wallet
    // address instead of our published HKDF-derived scan/encryption keys.
    let wallet_derived_seed = crate::crypto::x25519::ed25519_seed_to_x25519_seed(&wallet.seed_bytes());
    let wallet_derived_pub = crate::crypto::x25519::public_key_from_seed(&wallet_derived_seed);

    let mut new_count = 0i64;

    // Temporary diagnostic trail (2026-08-11) for a live, not-yet-root-caused
    // report: an alias-chat accept reply never completing on the receiving
    // side even after repeated Force Resync. Writes to
    // `log_sync_diagnostic` (see its doc comment — `%TEMP%\sealed-desktop-sync.log`,
    // no terminal needed to read it).
    log_sync_diagnostic(&format!("sync_incoming_messages: scanning {} candidate(s)", candidates.len()));

    for msg in candidates {
        if messages::has_message(conn, &msg.account_pubkey)? {
            continue;
        }
        if msg.sender_address == wallet.address {
            continue;
        }

        let mut is_for_me = crate::crypto::check_recipient_tag(&msg.sender_encryption_pubkey, &msg.recipient_tag, &sealed_keys.view_private_key);
        let mut used_wallet_derived = false;
        if !is_for_me {
            is_for_me = crate::crypto::check_recipient_tag(&msg.sender_encryption_pubkey, &msg.recipient_tag, &wallet_derived_seed);
            used_wallet_derived = is_for_me;
        }
        if !is_for_me {
            continue;
        }
        let _ = wallet_derived_pub; // only the private half is needed for decryption below
        log_sync_diagnostic(&format!(
            "sync_incoming_messages: tag matched for candidate from {} (tx {}), used_wallet_derived={used_wallet_derived}",
            msg.sender_address, msg.account_pubkey
        ));

        let Some(combined) = unpad_message(&msg.ciphertext) else {
            log_sync_diagnostic("sync_incoming_messages: unpad_message failed after tag match");
            continue;
        };
        let ciphertext = match split_ciphertexts(&combined) {
            Some((recipient_ct, _self_ct)) => recipient_ct,
            None => combined,
        };

        let decryption_seed = if used_wallet_derived { &wallet_derived_seed } else { &sealed_keys.encryption_private_key };
        let shared = crate::crypto::x25519::shared_secret_from_seed(decryption_seed, &msg.sender_encryption_pubkey);

        let pq_secret = if !msg.sender_address.is_empty() {
            contacts::get_contact_keys(conn, &msg.sender_address)?.pq_shared_secret
        } else {
            None
        };

        // Try hybrid (PQ + classical) first, fall back to classical-only —
        // handles senders who skipped/never completed the KEM handshake
        // even though we have a stale cached shared secret for them.
        let decrypted = match crate::crypto::decrypt_hybrid(&ciphertext, &shared, pq_secret.as_deref()) {
            Ok(d) => d,
            Err(_) => match crate::crypto::decrypt_hybrid(&ciphertext, &shared, None) {
                Ok(d) => d,
                Err(_) => {
                    log_sync_diagnostic(&format!(
                        "sync_incoming_messages: decrypt_hybrid failed (both with and without cached pq_secret={}) for {}",
                        pq_secret.is_some(),
                        msg.sender_address
                    ));
                    continue;
                }
            },
        };
        log_sync_diagnostic(&format!("sync_incoming_messages: decrypted {} byte(s) from {}", decrypted.len(), msg.sender_address));

        // Alias-chat invite/accept envelopes (Phase 7h) are dispatched here,
        // BEFORE gzip-decompress: they're never gzip/JSON-wrapped in the
        // first place (unlike every normal text DM), and gzip's magic bytes
        // (`0x1f 0x8b`) never collide with the envelopes' leading version
        // byte (`0x01`/`0x02`), so this ordering is unambiguous. See
        // `alias::invite_delivery`'s module doc comment.
        //
        // **Bug fixed 2026-08-11**: `handle_incoming_invite`/`handle_incoming_accept`
        // used to be called with `?`, propagating any error (a malformed
        // envelope, a transactional DB failure, ...) all the way up through
        // `apply_sync_result` and aborting the *entire* sync pass — meaning
        // one bad/incompatible alias candidate silently blocked every other
        // message in the same sync from being processed too. Every other
        // per-candidate failure in this loop is handled by `continue`, never
        // by bubbling up; these two calls broke that convention. Now logged
        // and skipped, matching the rest of this function.
        //
        // **Third bug fixed 2026-08-11**: neither branch used to touch
        // `new_count` at all — a successfully-recorded incoming invite or a
        // successfully-promoted alias contact was invisible to every caller
        // that only checks "did anything change" via this return value
        // (the frontend's manual "Sync now"/"Force resync" invalidation,
        // and the background tick's `messages-updated` event/notification
        // in `sync/mod.rs`, both gated on `new_count > 0`). See
        // `alias::invite_delivery::handle_incoming_accept`'s doc comment for
        // the full chain. Now counted like any other real sync outcome.
        match crate::alias::invite_delivery::classify(&decrypted) {
            crate::alias::invite_delivery::IncomingAliasEnvelope::Invite => {
                log_sync_diagnostic(&format!("sync_incoming_messages: classified as alias INVITE from {}", msg.sender_address));
                match crate::alias::invite_delivery::handle_incoming_invite(conn, &msg.sender_address, &decrypted, msg.timestamp) {
                    Ok(true) => {
                        log_sync_diagnostic("sync_incoming_messages: alias invite recorded successfully");
                        new_count += 1;
                    }
                    Ok(false) => log_sync_diagnostic("sync_incoming_messages: alias invite was already recorded (duplicate delivery)"),
                    Err(e) => {
                        let line = format!("[sync] failed to record incoming alias invite from {}: {e}", msg.sender_address);
                        eprintln!("{line}");
                        log_sync_diagnostic(&line);
                    }
                }
                continue;
            }
            crate::alias::invite_delivery::IncomingAliasEnvelope::Accept => {
                log_sync_diagnostic(&format!("sync_incoming_messages: classified as alias ACCEPT from {}", msg.sender_address));
                match crate::alias::invite_delivery::handle_incoming_accept(conn, &decrypted, msg.timestamp) {
                    Ok(true) => {
                        log_sync_diagnostic("sync_incoming_messages: alias accept matched a pending invite and promoted a new contact");
                        new_count += 1;
                    }
                    Ok(false) => log_sync_diagnostic("sync_incoming_messages: alias accept was a no-op (no match, or already established)"),
                    Err(e) => {
                        let line = format!("[sync] failed to complete incoming alias accept: {e}");
                        eprintln!("{line}");
                        log_sync_diagnostic(&line);
                    }
                }
                continue;
            }
            crate::alias::invite_delivery::IncomingAliasEnvelope::None => {}
        }

        let Ok(decompressed) = gzip_decompress(&decrypted) else { continue };
        let payload = parse_message_payload(&decompressed);

        messages::save_message(
            conn,
            &DecryptedMessage {
                id: msg.account_pubkey.clone(),
                sender_wallet: payload.sender_wallet,
                sender_username: payload.sender_username,
                recipient_wallet: wallet.address.clone(),
                recipient_username: payload.recipient_username,
                content: payload.content,
                timestamp: msg.timestamp,
                is_outgoing: false,
                on_chain_pubkey: msg.account_pubkey.clone(),
            },
        )?;
        new_count += 1;
    }

    Ok(new_count)
}

fn sync_outgoing_messages(
    conn: &Connection,
    wallet: &AlgorandWallet,
    sealed_keys: &SealedKeys,
    candidates: &[crate::chain::client::ChainMessage],
) -> Result<i64, MessagingError> {
    let mut new_count = 0i64;

    for msg in candidates {
        if messages::has_message(conn, &msg.account_pubkey)? {
            continue;
        }

        let Some(combined) = unpad_message(&msg.ciphertext) else { continue };
        let Some((_recipient_ct, self_ct)) = split_ciphertexts(&combined) else { continue };

        let shared = crate::crypto::x25519::shared_secret_from_seed(&sealed_keys.encryption_private_key, &msg.sender_encryption_pubkey);
        let Ok(decrypted) = crate::crypto::decrypt_hybrid(&self_ct, &shared, None) else { continue };
        let Ok(decompressed) = gzip_decompress(&decrypted) else { continue };
        let payload = parse_message_payload(&decompressed);

        messages::save_message(
            conn,
            &DecryptedMessage {
                id: msg.account_pubkey.clone(),
                sender_wallet: wallet.address.clone(),
                sender_username: payload.sender_username,
                recipient_wallet: payload.recipient_wallet.unwrap_or_else(|| "unknown".to_string()),
                recipient_username: payload.recipient_username,
                content: payload.content,
                timestamp: msg.timestamp,
                is_outgoing: true,
                on_chain_pubkey: msg.account_pubkey.clone(),
            },
        )?;
        new_count += 1;
    }

    Ok(new_count)
}

/// Parsed message JSON payload, permissive like `_parseMessagePayload`:
/// falls back to treating the whole decrypted string as plain content if
/// it isn't valid JSON, rather than erroring the whole sync pass.
struct ParsedPayload {
    sender_wallet: String,
    sender_username: Option<String>,
    recipient_wallet: Option<String>,
    recipient_username: Option<String>,
    content: String,
}

fn parse_message_payload(decompressed: &[u8]) -> ParsedPayload {
    let text = String::from_utf8_lossy(decompressed);
    match serde_json::from_str::<MessagePayload>(&text) {
        Ok(p) => ParsedPayload {
            sender_wallet: p.sender_wallet,
            sender_username: p.sender_username,
            recipient_wallet: Some(p.recipient_wallet),
            recipient_username: p.recipient_username,
            content: p.content,
        },
        Err(_) => ParsedPayload {
            sender_wallet: "unknown".to_string(),
            sender_username: None,
            recipient_wallet: None,
            recipient_username: None,
            content: text.into_owned(),
        },
    }
}

/// Clears the local message cache + sync state, in preparation for
/// [`finalize_force_resync`] repopulating it from a from-genesis fetch.
/// Mirrors the first part of `MessageService.forceResync` (minus the
/// indexer view-key re-registration step, which doesn't apply — no
/// indexer in this port's messaging sync layer).
///
/// **Call this only after [`fetch_sync_data`] (with `since_timestamp =
/// Some(0)`) has already succeeded** — despite the name, nothing about the
/// fetch actually depends on this having run first (the caller passes
/// `since_timestamp` explicitly; this function doesn't touch it). Calling
/// it before the fetch, as `commands::messaging::force_resync` used to,
/// left a real data-loss window: any failure during the fetch (network
/// hiccup, especially now that `query_indexer_transactions` can make many
/// sequential requests) wiped the in-memory cache with nothing to replace
/// it, for the rest of the running app session — see that command's doc
/// comment for the full incident.
pub fn prepare_force_resync(conn: &Connection) -> Result<(), MessagingError> {
    messages::clear(conn)?;
    sync_state::reset(conn)?;
    Ok(())
}

/// Sync-only half: apply the fetched full-sync data, then mark everything
/// read (a force-resync is a deliberate full reload, not a "catch up on
/// unread" — matches `MessageService.forceResync` calling
/// `markAllAsRead()` at the end).
pub fn finalize_force_resync(conn: &mut Connection, wallet: &AlgorandWallet, sealed_keys: &SealedKeys, fetch: &SyncFetchResult) -> Result<i64, MessagingError> {
    let count = apply_sync_result(conn, wallet, sealed_keys, fetch)?;
    messages::mark_all_as_read(conn)?;
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pad_unpad_round_trip() {
        let data = b"hello world, this is a test message payload";
        let padded = pad_message(data).unwrap();
        assert_eq!(padded.len() % PAD_ALIGNMENT, 0);
        assert_eq!(unpad_message(&padded).unwrap(), data);
    }

    #[test]
    fn pad_rejects_oversized_input() {
        assert!(pad_message(&vec![0u8; MAX_DATA_SIZE + 1]).is_err());
    }

    #[test]
    fn unpad_rejects_wrong_version() {
        let mut padded = pad_message(b"hi").unwrap();
        padded[0] = 0x01;
        assert!(unpad_message(&padded).is_none());
    }

    #[test]
    fn combine_split_ciphertexts_round_trip() {
        let recipient_ct = vec![1u8; 50];
        let self_ct = vec![2u8; 30];
        let combined = combine_ciphertexts(&recipient_ct, &self_ct);
        let (r, s) = split_ciphertexts(&combined).unwrap();
        assert_eq!(r, recipient_ct);
        assert_eq!(s, self_ct);
    }

    #[test]
    fn split_rejects_too_short_input() {
        assert!(split_ciphertexts(&[1, 2, 3]).is_none());
    }

    /// Regression guard for `send_raw_bytes_network`'s documented razor-thin
    /// size margin: a real invite envelope (the tighter of the two alias
    /// wire shapes), hybrid-encrypted and combined the same way
    /// `send_raw_bytes_network` does, must still fit under `MAX_DATA_SIZE`.
    /// If this ever fails, `encrypt_hybrid`'s overhead or the envelope
    /// format changed — `send_raw_bytes_network` would start throwing
    /// `MessagingError::TooLarge` on every invite send.
    #[test]
    fn invite_envelope_fits_under_max_data_size_after_combine() {
        let invite = crate::alias::envelope::InviteEnvelope {
            enc_pub: [1u8; 32],
            scan_pub: [2u8; 32],
            pq_pub: vec![3u8; crate::crypto::pq::PQ_PUBLIC_KEY_LEN],
        };
        let envelope_bytes = crate::alias::envelope::encode_invite_envelope(&invite);

        let shared = [9u8; 32];
        let recipient_ct = crate::crypto::encrypt_hybrid(&envelope_bytes, &shared, None).unwrap();
        let combined = combine_ciphertexts(&recipient_ct, &[]);

        assert!(combined.len() <= MAX_DATA_SIZE, "invite envelope combined size {} exceeds MAX_DATA_SIZE {}", combined.len(), MAX_DATA_SIZE);
        assert!(
            MAX_DATA_SIZE - combined.len() < 10,
            "margin grew unexpectedly ({} bytes free) — fine, just update the doc comment on send_raw_bytes_network",
            MAX_DATA_SIZE - combined.len()
        );
    }

    #[test]
    fn gzip_round_trip() {
        let data = b"the quick brown fox jumps over the lazy dog".repeat(20);
        let compressed = gzip_compress(&data).unwrap();
        assert!(compressed.len() < data.len());
        assert_eq!(gzip_decompress(&compressed).unwrap(), data);
    }

    #[test]
    fn kem_discovery_tag_is_deterministic_and_direction_sensitive() {
        let a_to_b = compute_kem_discovery_tag("WALLET_A", "WALLET_B");
        let a_to_b_again = compute_kem_discovery_tag("WALLET_A", "WALLET_B");
        let b_to_a = compute_kem_discovery_tag("WALLET_B", "WALLET_A");
        assert_eq!(a_to_b, a_to_b_again);
        assert_ne!(a_to_b, b_to_a);
    }

    #[test]
    fn parse_message_payload_falls_back_to_plain_text_on_invalid_json() {
        let parsed = parse_message_payload(b"not json at all");
        assert_eq!(parsed.content, "not json at all");
        assert_eq!(parsed.sender_wallet, "unknown");
    }

    #[test]
    fn parse_message_payload_reads_well_formed_json() {
        let json = br#"{"sender_wallet":"A","recipient_wallet":"B","content":"hi","timestamp":123}"#;
        let parsed = parse_message_payload(json);
        assert_eq!(parsed.sender_wallet, "A");
        assert_eq!(parsed.recipient_wallet.as_deref(), Some("B"));
        assert_eq!(parsed.content, "hi");
    }
}
