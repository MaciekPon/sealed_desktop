//! Sealed unified-contract chain client, ported from `sealed_chain_client.dart`.
//!
//! Sends are 2-txn groups: a `TreasuryEscrow` self-payment (txn 0, fee
//! bumped to cover both) and an app-call NoOp targeting `SEALED_APP_ID`
//! (txn 1, fee=0). See `chain::escrow` and `chain::txn` for the group
//! construction this builds on.
//!
//! Fix applied relative to the Dart source: `sendMessage` there never
//! receives or embeds the sender's ephemeral X25519 pubkey into the framed
//! on-chain payload (leaves the first 32 bytes zeroed) even though the read
//! path (`fetch_messages` below, ported from `_fetchAppCallMessages`) and
//! the mobile test suite's own golden vector both expect a real pubkey
//! there — see the chat message for the analysis. This port's
//! [`SealedChainClient::send_message`] takes `sender_ephemeral_pubkey`
//! explicitly and embeds it correctly.

use base64::Engine;
use sha2::{Digest, Sha256};
use thiserror::Error;
use url::Url;

use crate::ohttp::client::{OhttpError, OhttpHttpClient};

use super::escrow::TreasuryEscrowSigner;
use super::msgpack::{encode_abi_dynamic_bytes, Field};
use super::txn::{
    abi_selector, build_app_call_txn_with_boxes, build_escrow_self_pay_txn,
    compute_group_id, compute_tx_id, encode_signed_tx_with_ed25519, encode_simulate_request,
    SuggestedParams, TxnFields,
};
use super::user_state::{commitment_box_key, decode_user_state, is_zero32, name_box_key, wallet_box_key};
use super::wallet::AlgorandWallet;

#[derive(Debug, Error)]
pub enum ChainError {
    #[error("No credits available. Redeem a code to send messages.")]
    NoCredits,
    #[error("No account box found. Redeem a code first.")]
    NoBox,
    #[error("Username already taken.")]
    NameTaken,
    #[error("invalid username format ({code}): {message}")]
    BadUsernameFormat { code: &'static str, message: &'static str },
    #[error("Invalid redeem code.")]
    BadRedeemCode,
    #[error("Insufficient balance for MBR.")]
    MbrShortfall,
    #[error("User not found")]
    UserNotFound,
    #[error("{0}")]
    Generic(String),
    // Never constructed today — `Session` always holds a loaded wallet by
    // the time any `SealedChainClient` method runs. Kept for the day a
    // caller needs to distinguish this from `Generic`.
    #[allow(dead_code)]
    #[error("wallet not loaded")]
    WalletNotLoaded,
    #[error("validation error: {0}")]
    Validation(String),
    #[error("ohttp error: {0}")]
    Ohttp(#[from] OhttpError),
    #[error("unexpected response shape: {0}")]
    UnexpectedResponse(String),
}

pub struct CreditCost {
    pub current: u64,
    pub after: u64,
}

pub struct UserProfile {
    pub wallet_address: String,
    pub username: Option<String>,
    pub encryption_pubkey: [u8; 32],
    pub scan_pubkey: [u8; 32],
    // Decoded from chain alongside `pq_pubkey_hash` for completeness, but
    // callers only ever compare the hash (staleness/freshness checks) — the
    // raw key isn't needed until KEM encapsulation, which reads it fresh
    // from `contacts_cache` instead of this transient struct.
    #[allow(dead_code)]
    pub pq_public_key: Option<Vec<u8>>,
    pub pq_pubkey_hash: Option<[u8; 32]>,
}

/// One decoded `sendMessage` app-call txn from the indexer, as returned by
/// [`SealedChainClient::fetch_messages`] / `fetch_messages_from_sender`.
pub struct ChainMessage {
    pub account_pubkey: String,
    pub recipient_tag: [u8; 32],
    pub ciphertext: Vec<u8>,
    pub sender_encryption_pubkey: [u8; 32],
    pub timestamp: i64,
    pub sender_address: String,
}

pub struct SealedChainClient {
    sealed_app_id: u64,
    /// Algod target reached through `ohttp` — see
    /// `constants::ALGO_ALGOD_TARGET_URL`'s doc comment for why this isn't
    /// the direct AlgoNode host.
    algod_url: String,
    indexer_url: String,
    ohttp: OhttpHttpClient,
}

impl SealedChainClient {
    pub fn new(sealed_app_id: u64, algod_url: impl Into<String>, indexer_url: impl Into<String>) -> Self {
        Self {
            sealed_app_id,
            algod_url: algod_url.into(),
            indexer_url: indexer_url.into(),
            ohttp: OhttpHttpClient::new_with_bundled_config(
                crate::constants::OHTTP_GATEWAY_CONFIG_URL,
                crate::constants::OHTTP_RELAY_URL,
                crate::constants::OHTTP_BUNDLED_CONFIG_HEX,
            ),
        }
    }

    // ------------------------------------------------------------------
    // sendMessage
    // ------------------------------------------------------------------

    /// Submit `sendMessage(recipientTag, framed)`. `framed` =
    /// `senderEphemeralPubkey(32) || ciphertext`. Returns the app-call
    /// (txn 1) transaction ID.
    pub async fn send_message(
        &self,
        wallet: &AlgorandWallet,
        escrow: &TreasuryEscrowSigner,
        recipient_tag: &[u8; 32],
        sender_ephemeral_pubkey: &[u8; 32],
        ciphertext: &[u8],
    ) -> Result<String, ChainError> {
        let mut framed = sender_ephemeral_pubkey.to_vec();
        framed.extend_from_slice(ciphertext);

        let params = self.get_suggested_params().await?;
        let sender_pubkey = wallet.public_key_bytes();

        let escrow_txn = build_escrow_self_pay_txn(&escrow.address_pubkey, params.min_fee * 2, &params);
        let selector = abi_selector("sendMessage(byte[32],byte[])void");
        let app_args = vec![recipient_tag.to_vec(), encode_abi_dynamic_bytes(&framed)];
        // `sendMessage` calls `spendOneCredit`, which reads and rewrites the
        // sender's own `w:<pubkey>` UserState box — must be declared or the
        // AVM rejects with "invalid Box reference" (missing from this call
        // until 2026-08-06; caught by a live mainnet send failing exactly
        // that way).
        let boxes = vec![(0u64, wallet_box_key(&sender_pubkey))];
        let app_call_txn = build_app_call_txn_with_boxes(&sender_pubkey, self.sealed_app_id, selector, app_args, boxes, 0, &params);

        self.sign_and_submit_group(escrow_txn, app_call_txn, wallet, escrow).await
    }

    // ------------------------------------------------------------------
    // claimUsername / releaseUsername / redeem / publishKeys
    // ------------------------------------------------------------------

    pub async fn claim_username(
        &self,
        wallet: &AlgorandWallet,
        escrow: &TreasuryEscrowSigner,
        username: &str,
        old_name: Option<&str>,
    ) -> Result<String, ChainError> {
        let name_bytes = username.as_bytes();
        if name_bytes.is_empty() || name_bytes.len() > 32 {
            return Err(ChainError::Validation("username must be 1..32 bytes utf-8".into()));
        }

        let credits = self.get_credits(wallet, &wallet.address).await?;
        if credits < 1 {
            return Err(ChainError::NoCredits);
        }

        let sender_pubkey = wallet.public_key_bytes();
        let name_box = name_box_key(name_bytes);
        let wallet_box = wallet_box_key(&sender_pubkey);
        let mut boxes = vec![(0u64, name_box), (0u64, wallet_box)];
        if let Some(old) = old_name.filter(|s| !s.is_empty()) {
            boxes.push((0u64, name_box_key(old.as_bytes())));
        }

        let params = self.get_suggested_params().await?;
        // `claimUsername` calls `ensureBudget(2400, OpUpFeeSource.GroupCredit)`
        // — the rename walk (batches + name bytes + 2 box ops + sha256) can
        // exceed the single-call 700-op budget, so the contract spawns ~3
        // opup inner-appl txns funded from this group's fee surplus. Pool
        // 5× minFee: escrow leg + app-call leg + 3 opup itxns — matches
        // `sealed_chain_client.dart`'s `claimUsername` exactly.
        let escrow_txn = build_escrow_self_pay_txn(&escrow.address_pubkey, params.min_fee * 5, &params);
        let selector = abi_selector("claimUsername(byte[])void");
        let app_call_txn = build_app_call_txn_with_boxes(
            &sender_pubkey,
            self.sealed_app_id,
            selector,
            vec![encode_abi_dynamic_bytes(name_bytes)],
            boxes,
            0,
            &params,
        );

        self.sign_and_submit_group(escrow_txn, app_call_txn, wallet, escrow).await
    }

    pub async fn release_username(
        &self,
        wallet: &AlgorandWallet,
        escrow: &TreasuryEscrowSigner,
        old_name: Option<&str>,
    ) -> Result<String, ChainError> {
        let credits = self.get_credits(wallet, &wallet.address).await?;
        if credits < 1 {
            return Err(ChainError::NoCredits);
        }

        let sender_pubkey = wallet.public_key_bytes();
        let mut boxes = vec![(0u64, wallet_box_key(&sender_pubkey))];
        if let Some(old) = old_name.filter(|s| !s.is_empty()) {
            boxes.push((0u64, name_box_key(old.as_bytes())));
        }

        let params = self.get_suggested_params().await?;
        // Same `ensureBudget(2400, OpUpFeeSource.GroupCredit)` reasoning as
        // `claim_username` — release walks batches + decodes username +
        // hashes + 1 box delete. Pool 5× minFee to match.
        let escrow_txn = build_escrow_self_pay_txn(&escrow.address_pubkey, params.min_fee * 5, &params);
        let selector = abi_selector("releaseUsername()void");
        let app_call_txn =
            build_app_call_txn_with_boxes(&sender_pubkey, self.sealed_app_id, selector, vec![], boxes, 0, &params);

        self.sign_and_submit_group(escrow_txn, app_call_txn, wallet, escrow).await
    }

    /// Dry-run credit cost for any 1-credit operation. No txn submitted.
    pub async fn estimate_credit_cost(&self, wallet: &AlgorandWallet) -> Result<CreditCost, ChainError> {
        let current = self.get_credits(wallet, &wallet.address).await?;
        if current < 1 {
            return Err(ChainError::NoCredits);
        }
        Ok(CreditCost { current, after: current - 1 })
    }

    /// Submit `redeem(preimage, username)`. `username` may be empty
    /// (deferred claim — call [`Self::claim_username`] separately).
    pub async fn redeem(
        &self,
        wallet: &AlgorandWallet,
        escrow: &TreasuryEscrowSigner,
        preimage: &[u8; 16],
        username: &str,
    ) -> Result<String, ChainError> {
        let name_bytes = username.as_bytes();
        if name_bytes.len() > 32 {
            return Err(ChainError::Validation("username must be <= 32 bytes utf-8".into()));
        }

        let sender_pubkey = wallet.public_key_bytes();
        let mut boxes = vec![(0u64, commitment_box_key(preimage)), (0u64, wallet_box_key(&sender_pubkey))];
        if !name_bytes.is_empty() {
            boxes.push((0u64, name_box_key(name_bytes)));
        }

        let params = self.get_suggested_params().await?;
        // Pool 3x minFee: escrow leg + app-call leg + 1 potential inner-txn
        // — the contract seeds the caller's MBR via `itxn.payment{fee:0}`
        // (fee drawn from this group's surplus) when the wallet's balance
        // is below ACCOUNT_MBR_MIN, which every brand-new 0-ALGO wallet's
        // first redeem hits. Without this headroom the group fee is too
        // small and `itxn_submit` fails. Also floors at 1000µA like
        // `sealed_chain_client.dart`'s `redeem`, in case algod ever reports
        // a lower suggested min-fee than the protocol minimum.
        let per_leg_fee = params.min_fee.max(1000);
        let escrow_txn = build_escrow_self_pay_txn(&escrow.address_pubkey, per_leg_fee * 3, &params);
        let selector = abi_selector("redeem(byte[],byte[])void");
        let app_call_txn = build_app_call_txn_with_boxes(
            &sender_pubkey,
            self.sealed_app_id,
            selector,
            vec![encode_abi_dynamic_bytes(preimage), encode_abi_dynamic_bytes(name_bytes)],
            boxes,
            0,
            &params,
        );

        self.sign_and_submit_group(escrow_txn, app_call_txn, wallet, escrow).await
    }

    /// Publish all cryptographic keys in a single txn.
    pub async fn publish_keys(
        &self,
        wallet: &AlgorandWallet,
        escrow: &TreasuryEscrowSigner,
        encryption_pubkey: &[u8; 32],
        scan_pubkey: &[u8; 32],
        pq_pubkey: &[u8],
    ) -> Result<String, ChainError> {
        if pq_pubkey.len() < 32 || pq_pubkey.len() > 2048 {
            return Err(ChainError::Validation("pqPubkey must be 32-2048 bytes".into()));
        }

        let profile = self.get_user_by_wallet(&wallet.address).await?;
        if profile.is_none() {
            return Err(ChainError::NoBox);
        }
        let credits = self.get_credits(wallet, &wallet.address).await?;
        if credits < 1 {
            return Err(ChainError::NoCredits);
        }

        let params = self.get_suggested_params().await?;
        let sender_pubkey = wallet.public_key_bytes();
        let escrow_txn = build_escrow_self_pay_txn(&escrow.address_pubkey, params.min_fee * 2, &params);
        let selector = abi_selector("publishKeys(byte[32],byte[32],byte[])void");
        let app_args = vec![
            encryption_pubkey.to_vec(),
            scan_pubkey.to_vec(),
            encode_abi_dynamic_bytes(pq_pubkey),
        ];
        // `publishKeys` also calls `spendOneCredit`, which touches the
        // sender's own `w:<pubkey>` box — same missing-box-reference bug as
        // `send_message` (fixed together, 2026-08-06).
        let boxes = vec![(0u64, wallet_box_key(&sender_pubkey))];
        let app_call_txn = build_app_call_txn_with_boxes(&sender_pubkey, self.sealed_app_id, selector, app_args, boxes, 0, &params);

        self.sign_and_submit_group(escrow_txn, app_call_txn, wallet, escrow).await
    }

    /// Publish this wallet's keys only if they're missing or differ from
    /// what's already on-chain. Returns `Ok(None)` — a benign no-op, not an
    /// error — if there's no `w:` box yet or the account has 0 credits,
    /// mirroring `publishKeysIfStale`'s early returns in
    /// `sealed_chain_client.dart`. Intended to be called opportunistically
    /// (e.g. after unlock, or right after a `redeem()` that just created the
    /// box), not gated on the caller already knowing the account is ready.
    pub async fn publish_keys_if_stale(
        &self,
        wallet: &AlgorandWallet,
        escrow: &TreasuryEscrowSigner,
        encryption_pubkey: &[u8; 32],
        scan_pubkey: &[u8; 32],
        pq_pubkey: &[u8],
    ) -> Result<Option<String>, ChainError> {
        let Some(profile) = self.get_user_by_wallet(&wallet.address).await? else {
            return Ok(None);
        };

        let pq_pubkey_hash: [u8; 32] = Sha256::digest(pq_pubkey).into();
        if !Self::keys_are_stale(&profile, encryption_pubkey, scan_pubkey, &pq_pubkey_hash) {
            return Ok(None);
        }

        match self.publish_keys(wallet, escrow, encryption_pubkey, scan_pubkey, pq_pubkey).await {
            Ok(tx_id) => Ok(Some(tx_id)),
            Err(ChainError::NoCredits) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// True if the on-chain `profile` doesn't yet match the locally-derived
    /// key material — i.e. a [`Self::publish_keys`] call is needed.
    fn keys_are_stale(
        profile: &UserProfile,
        encryption_pubkey: &[u8; 32],
        scan_pubkey: &[u8; 32],
        pq_pubkey_hash: &[u8; 32],
    ) -> bool {
        profile.encryption_pubkey != *encryption_pubkey
            || profile.scan_pubkey != *scan_pubkey
            || profile.pq_pubkey_hash.as_ref() != Some(pq_pubkey_hash)
    }

    // ------------------------------------------------------------------
    // Profile reads (algod box)
    // ------------------------------------------------------------------

    /// Read the `w:<wallet>` UserState box. `pq_public_key` is always
    /// `None` — callers fetch it lazily via the indexer and verify against
    /// `pq_pubkey_hash`.
    pub async fn get_user_by_wallet(&self, wallet_address: &str) -> Result<Option<UserProfile>, ChainError> {
        let pubkey = super::address::decode_address(wallet_address)
            .ok_or_else(|| ChainError::Validation("invalid wallet address".into()))?;
        let box_key = wallet_box_key(&pubkey);
        let Some(raw) = self.read_box(&box_key).await? else {
            return Ok(None);
        };
        let state = decode_user_state(&raw).map_err(|e| ChainError::UnexpectedResponse(e.to_string()))?;
        Ok(Some(UserProfile {
            wallet_address: wallet_address.to_string(),
            username: if state.username.is_empty() {
                None
            } else {
                Some(String::from_utf8_lossy(&state.username).into_owned())
            },
            encryption_pubkey: state.encryption_pubkey,
            scan_pubkey: state.scan_pubkey,
            pq_public_key: None,
            pq_pubkey_hash: if is_zero32(&state.pq_pubkey_hash) {
                None
            } else {
                Some(state.pq_pubkey_hash)
            },
        }))
    }

    /// Read `amount` (microAlgos) from algod's `/v2/accounts/{address}`.
    /// `exclude=all` skips assets/apps/created-* in the response — this
    /// caller only wants the balance, and a busy wallet's full account
    /// record can otherwise be sizeable.
    pub async fn get_account_balance(&self, wallet_address: &str) -> Result<u64, ChainError> {
        let mut url = Url::parse(&format!("{}/v2/accounts/{}", self.algod_url, wallet_address))
            .map_err(|e| ChainError::UnexpectedResponse(e.to_string()))?;
        url.query_pairs_mut().append_pair("exclude", "all");

        let resp = self.ohttp.get(&url, &[]).await?;
        if !resp.is_success() {
            let message = String::from_utf8_lossy(&resp.body).into_owned();
            return Err(ChainError::UnexpectedResponse(format!("algod status {}: {message}", resp.status_code)));
        }
        let value: serde_json::Value =
            serde_json::from_slice(&resp.body).map_err(|e| ChainError::UnexpectedResponse(e.to_string()))?;
        value
            .get("amount")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| ChainError::UnexpectedResponse("missing amount".into()))
    }

    /// Resolve username -> wallet via the `n:` box, then fetch the profile.
    pub async fn get_user_by_username(&self, name: &str) -> Result<Option<UserProfile>, ChainError> {
        let name_bytes = name.to_lowercase().into_bytes();
        let name_box = name_box_key(&name_bytes);
        let Some(raw) = self.read_box(&name_box).await? else {
            return Ok(None);
        };
        if raw.len() < 32 {
            return Ok(None);
        }
        let mut pubkey = [0u8; 32];
        pubkey.copy_from_slice(&raw[..32]);
        let wallet_address = super::address::encode_address(&pubkey);
        self.get_user_by_wallet(&wallet_address).await
    }

    /// Readonly `getCredits(address)uint64` via algod simulate. Returns 0
    /// when the credit box does not exist. `wallet` supplies the simulate
    /// txn's sender (algod requires a syntactically valid sender even for
    /// an unsigned, zero-fee simulate call).
    pub async fn get_credits(&self, wallet: &AlgorandWallet, address: &str) -> Result<u64, ChainError> {
        let addr_pubkey =
            super::address::decode_address(address).ok_or_else(|| ChainError::Validation("invalid address".into()))?;
        let sender_pubkey = wallet.public_key_bytes();
        let params = self.get_suggested_params().await?;
        let selector = abi_selector("getCredits(address)uint64");
        let app_call_txn = build_app_call_txn_with_boxes(
            &sender_pubkey,
            self.sealed_app_id,
            selector,
            vec![addr_pubkey.to_vec()],
            vec![(0, wallet_box_key(&addr_pubkey))],
            params.min_fee,
            &params,
        );
        let payload = self.simulate_abi_return(&app_call_txn).await?;
        match payload {
            Some(bytes) if bytes.len() >= 8 => {
                let mut arr = [0u8; 8];
                arr.copy_from_slice(&bytes[..8]);
                Ok(u64::from_be_bytes(arr))
            }
            _ => Ok(0),
        }
    }

    // ------------------------------------------------------------------
    // Indexer queries
    // ------------------------------------------------------------------

    pub async fn fetch_messages(
        &self,
        since_timestamp: Option<i64>,
        limit: u32,
    ) -> Result<Vec<ChainMessage>, ChainError> {
        self.fetch_app_call_messages(None, since_timestamp, limit).await
    }

    pub async fn fetch_messages_from_sender(
        &self,
        sender_address: &str,
        since_timestamp: Option<i64>,
        limit: u32,
    ) -> Result<Vec<ChainMessage>, ChainError> {
        self.fetch_app_call_messages(Some(sender_address), since_timestamp, limit).await
    }

    async fn fetch_app_call_messages(
        &self,
        sender_address: Option<&str>,
        since_timestamp: Option<i64>,
        limit: u32,
    ) -> Result<Vec<ChainMessage>, ChainError> {
        let selector = abi_selector("sendMessage(byte[32],byte[])void");
        let selector_b64 = base64::engine::general_purpose::STANDARD.encode(selector);

        let txs = self.query_indexer_transactions(sender_address, since_timestamp, limit).await?;
        let mut messages = Vec::new();
        for tx in txs {
            let Some(app_tx) = tx.get("application-transaction") else { continue };
            let Some(args) = app_tx.get("application-args").and_then(|v| v.as_array()) else { continue };
            if args.len() < 3 {
                continue;
            }
            if args[0].as_str() != Some(selector_b64.as_str()) {
                continue;
            }
            let Some(parsed) = Self::parse_message_args(args) else { continue };
            messages.push(ChainMessage {
                account_pubkey: tx.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                recipient_tag: parsed.0,
                ciphertext: parsed.1,
                sender_encryption_pubkey: parsed.2,
                timestamp: tx.get("round-time").and_then(|v| v.as_i64()).unwrap_or(0),
                sender_address: tx.get("sender").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            });
        }
        messages.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        Ok(messages)
    }

    fn parse_message_args(args: &[serde_json::Value]) -> Option<([u8; 32], Vec<u8>, [u8; 32])> {
        let recipient_tag_raw = base64::engine::general_purpose::STANDARD.decode(args[1].as_str()?).ok()?;
        if recipient_tag_raw.len() != 32 {
            return None;
        }
        let mut recipient_tag = [0u8; 32];
        recipient_tag.copy_from_slice(&recipient_tag_raw);

        let framed_arg = base64::engine::general_purpose::STANDARD.decode(args[2].as_str()?).ok()?;
        if framed_arg.len() < 2 {
            return None;
        }
        let payload_len = u16::from_be_bytes([framed_arg[0], framed_arg[1]]) as usize;
        if framed_arg.len() < 2 + payload_len {
            return None;
        }
        let framed = &framed_arg[2..2 + payload_len];
        if framed.len() < 32 {
            return None;
        }
        let mut sender_encryption_pubkey = [0u8; 32];
        sender_encryption_pubkey.copy_from_slice(&framed[..32]);
        let ciphertext = framed[32..].to_vec();

        Some((recipient_tag, ciphertext, sender_encryption_pubkey))
    }

    /// Pages through every `/v2/transactions` result for the given filters,
    /// following the indexer's `next-token` until exhausted. Mirrors
    /// `SealedChainClient._fetchTxnPages` in `sealed_chain_client.dart` —
    /// its doc comment records exactly the bug this ports the fix for: "A
    /// single un-paginated `limit`-capped query previously dropped any
    /// message beyond the first page whenever app-wide traffic in the
    /// window exceeded the limit." `fetch_messages`'s unfiltered global scan
    /// (used for both incoming-message and KEM-handshake candidates) is
    /// exactly this scenario on a mainnet app with any real traffic — a
    /// single page silently loses any older message once app-wide
    /// `sendMessage` volume exceeds `limit` (200), with no error to signal
    /// it. `limit` here still bounds each individual page's size (indexer
    /// requests it as `?limit=`), not the total result count.
    async fn query_indexer_transactions(
        &self,
        sender_address: Option<&str>,
        since_timestamp: Option<i64>,
        limit: u32,
    ) -> Result<Vec<serde_json::Value>, ChainError> {
        const MAX_PAGES: u32 = 100;

        let mut base_query: Vec<(&str, String)> = vec![
            ("application-id", self.sealed_app_id.to_string()),
            ("tx-type", "appl".to_string()),
            ("limit", limit.to_string()),
        ];
        if let Some(addr) = sender_address {
            base_query.push(("address", addr.to_string()));
        }
        if let Some(ts) = since_timestamp.filter(|t| *t > 0) {
            let dt = chrono_after_time(ts);
            base_query.push(("after-time", dt));
        }

        let mut all = Vec::new();
        let mut next_token: Option<String> = None;
        let mut pages = 0u32;

        loop {
            let mut query = base_query.clone();
            if let Some(next) = &next_token {
                if !next.is_empty() {
                    query.push(("next", next.clone()));
                }
            }

            let mut url = Url::parse(&format!("{}/v2/transactions", self.indexer_url))
                .map_err(|e| ChainError::UnexpectedResponse(e.to_string()))?;
            url.query_pairs_mut().extend_pairs(&query);

            let resp = self.ohttp.get(&url, &[]).await?;
            if !resp.is_success() {
                return Err(ChainError::UnexpectedResponse(format!("indexer status {}", resp.status_code)));
            }
            let body: serde_json::Value =
                serde_json::from_slice(&resp.body).map_err(|e| ChainError::UnexpectedResponse(e.to_string()))?;

            let txs = body.get("transactions").and_then(|v| v.as_array()).cloned().unwrap_or_default();
            let page_was_empty = txs.is_empty();
            all.extend(txs);

            next_token = body.get("next-token").and_then(|v| v.as_str()).map(str::to_string);
            pages += 1;

            let has_next = next_token.as_deref().is_some_and(|t| !t.is_empty());
            if page_was_empty || !has_next || pages >= MAX_PAGES {
                break;
            }
        }

        Ok(all)
    }

    // ------------------------------------------------------------------
    // Internal: algod
    // ------------------------------------------------------------------

    async fn get_suggested_params(&self) -> Result<SuggestedParams, ChainError> {
        let url = Url::parse(&format!("{}/v2/transactions/params", self.algod_url))
            .map_err(|e| ChainError::UnexpectedResponse(e.to_string()))?;
        let resp = self.ohttp.get(&url, &[]).await?;
        if !resp.is_success() {
            return Err(ChainError::UnexpectedResponse(format!("algod status {}", resp.status_code)));
        }
        let body: serde_json::Value =
            serde_json::from_slice(&resp.body).map_err(|e| ChainError::UnexpectedResponse(e.to_string()))?;
        let min_fee = body.get("min-fee").and_then(|v| v.as_u64()).unwrap_or(1000);
        let last_round = body
            .get("last-round")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| ChainError::UnexpectedResponse("missing last-round".into()))?;
        let genesis_id = body
            .get("genesis-id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ChainError::UnexpectedResponse("missing genesis-id".into()))?
            .to_string();
        let genesis_hash_b64 = body
            .get("genesis-hash")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ChainError::UnexpectedResponse("missing genesis-hash".into()))?;
        let genesis_hash_bytes = base64::engine::general_purpose::STANDARD
            .decode(genesis_hash_b64)
            .map_err(|e| ChainError::UnexpectedResponse(e.to_string()))?;
        let mut genesis_hash = [0u8; 32];
        if genesis_hash_bytes.len() != 32 {
            return Err(ChainError::UnexpectedResponse("genesis-hash wrong length".into()));
        }
        genesis_hash.copy_from_slice(&genesis_hash_bytes);

        Ok(SuggestedParams {
            min_fee,
            first_valid: last_round,
            last_valid: last_round + 1000,
            genesis_id,
            genesis_hash,
        })
    }

    /// Algod box GET. Returns `None` on 404.
    async fn read_box(&self, box_key: &[u8]) -> Result<Option<Vec<u8>>, ChainError> {
        let name_param = format!("b64:{}", base64::engine::general_purpose::STANDARD.encode(box_key));
        let mut url = Url::parse(&format!("{}/v2/applications/{}/box", self.algod_url, self.sealed_app_id))
            .map_err(|e| ChainError::UnexpectedResponse(e.to_string()))?;
        url.query_pairs_mut().append_pair("name", &name_param);

        let resp = self.ohttp.get(&url, &[]).await?;
        if resp.status_code == 404 {
            return Ok(None);
        }
        if !resp.is_success() {
            return Err(ChainError::UnexpectedResponse(format!("algod status {}", resp.status_code)));
        }
        let body: serde_json::Value =
            serde_json::from_slice(&resp.body).map_err(|e| ChainError::UnexpectedResponse(e.to_string()))?;
        let Some(value_b64) = body.get("value").and_then(|v| v.as_str()) else {
            return Ok(None);
        };
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(value_b64)
            .map_err(|e| ChainError::UnexpectedResponse(e.to_string()))?;
        Ok(Some(decoded))
    }

    async fn submit_group(&self, signed_blob: &[u8]) -> Result<(), ChainError> {
        let url = Url::parse(&format!("{}/v2/transactions", self.algod_url))
            .map_err(|e| ChainError::UnexpectedResponse(e.to_string()))?;
        let headers = [("Content-Type".to_string(), "application/x-binary".to_string())];
        let resp = self.ohttp.post(&url, &headers, Some(signed_blob)).await?;
        if !resp.is_success() {
            // Unlike the old `reqwest::Response::error_for_status()` call
            // this replaces, we now actually have the response body — algod
            // embeds the TEAL `assert` reason there, so classify it here
            // instead of silently discarding it.
            let message = String::from_utf8_lossy(&resp.body).into_owned();
            return Err(self.classify_contract_error_body(&message));
        }
        Ok(())
    }

    /// Simulate an unsigned app-call, return the ABI return payload (bytes
    /// after the 4-byte `0x151f7c75` return prefix), or `None` if absent.
    ///
    /// Sent as raw msgpack (`Content-Type: application/msgpack`) via
    /// [`encode_simulate_request`] — see its doc comment for why the more
    /// obvious JSON-with-base64-txns shape doesn't work against live algod.
    async fn simulate_abi_return(&self, app_call_txn: &TxnFields) -> Result<Option<Vec<u8>>, ChainError> {
        let body_bytes = encode_simulate_request(app_call_txn);
        let url = Url::parse(&format!("{}/v2/transactions/simulate", self.algod_url))
            .map_err(|e| ChainError::UnexpectedResponse(e.to_string()))?;
        let headers = [("Content-Type".to_string(), "application/msgpack".to_string())];
        let resp = self.ohttp.post(&url, &headers, Some(&body_bytes)).await?;
        if !resp.is_success() {
            let message = String::from_utf8_lossy(&resp.body).into_owned();
            return Err(ChainError::UnexpectedResponse(format!("algod simulate status {}: {message}", resp.status_code)));
        }
        let value: serde_json::Value =
            serde_json::from_slice(&resp.body).map_err(|e| ChainError::UnexpectedResponse(e.to_string()))?;
        let logs = value
            .pointer("/txn-groups/0/txn-results/0/txn-result/logs")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let Some(last) = logs.last().and_then(|v| v.as_str()) else {
            return Ok(None);
        };
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(last)
            .map_err(|e| ChainError::UnexpectedResponse(e.to_string()))?;
        if decoded.len() < 4 {
            return Ok(None);
        }
        Ok(Some(decoded[4..].to_vec()))
    }

    /// Sign both txn legs and submit the group. Returns the app-call TxID.
    async fn sign_and_submit_group(
        &self,
        mut escrow_txn: TxnFields,
        mut app_call_txn: TxnFields,
        wallet: &AlgorandWallet,
        escrow: &TreasuryEscrowSigner,
    ) -> Result<String, ChainError> {
        let group_id = compute_group_id(&[clone_fields(&escrow_txn), clone_fields(&app_call_txn)]);
        escrow_txn.push(("grp", Field::Bin(group_id.to_vec())));
        app_call_txn.push(("grp", Field::Bin(group_id.to_vec())));

        let escrow_signed = escrow.encode_signed_txn(&escrow_txn);
        let app_call_to_sign = super::txn::encode_tx_for_signing(&app_call_txn);
        let sig = wallet.sign(&app_call_to_sign);
        let app_call_signed = encode_signed_tx_with_ed25519(&app_call_txn, &sig);

        let mut blob = escrow_signed;
        blob.extend_from_slice(&app_call_signed);

        let tx_id = compute_tx_id(&app_call_txn);
        self.submit_group(&blob).await?;
        Ok(tx_id)
    }

    /// Translate a TEAL `assert` reason embedded in the algod error body
    /// into a typed [`ChainError`]. Called directly from [`Self::submit_group`]
    /// on a non-success response, since that's the only place the raw body
    /// text is available.
    fn classify_contract_error_body(&self, body: &str) -> ChainError {
        if body.contains("TAKEN") {
            return ChainError::NameTaken;
        }
        if body.contains("NO_CREDITS") {
            return ChainError::NoCredits;
        }
        if body.contains("NO_BOX") || body.contains("NOT_FOUND") || body.contains("KEYS_UNSET") {
            return ChainError::UserNotFound;
        }
        if body.contains("BAD_PREIMAGE") || body.contains("BAD_CODE") {
            return ChainError::BadRedeemCode;
        }
        if body.contains("INSUFFICIENT_MBR") || body.contains("overspend") {
            return ChainError::MbrShortfall;
        }
        if body.contains("LEADING_UNDERSCORE") {
            return ChainError::BadUsernameFormat { code: "LEADING_UNDERSCORE", message: "Username cannot start with \"_\"." };
        }
        if body.contains("LEADING_DIGIT") {
            return ChainError::BadUsernameFormat { code: "LEADING_DIGIT", message: "Username cannot start with a digit." };
        }
        if body.contains("TRAILING_UNDERSCORE") {
            return ChainError::BadUsernameFormat { code: "TRAILING_UNDERSCORE", message: "Username cannot end with \"_\"." };
        }
        if body.contains("BAD_CHAR") {
            return ChainError::BadUsernameFormat { code: "BAD_CHAR", message: "Username must be a-z, 0-9, or \"_\"." };
        }
        if body.contains("BAD_LEN") {
            return ChainError::BadUsernameFormat { code: "BAD_LEN", message: "Username length out of range." };
        }
        ChainError::Generic(body.to_string())
    }
}

fn clone_fields(fields: &TxnFields) -> TxnFields {
    fields
        .iter()
        .map(|(k, v)| {
            let cloned = match v {
                Field::UInt(n) => Field::UInt(*n),
                Field::Str(s) => Field::Str(s.clone()),
                Field::Bin(b) => Field::Bin(b.clone()),
                Field::BinList(l) => Field::BinList(l.clone()),
                Field::Boxes(b) => Field::Boxes(b.clone()),
            };
            (*k, cloned)
        })
        .collect()
}

/// Minimal RFC 3339 UTC timestamp for algod's indexer `after-time` query
/// param, built from a Unix-seconds timestamp without pulling in a date/time
/// crate (`chrono`) just for this.
fn chrono_after_time(unix_seconds: i64) -> String {
    let days_since_epoch = unix_seconds.div_euclid(86400);
    let secs_of_day = unix_seconds.rem_euclid(86400);
    let (year, month, day) = civil_from_days(days_since_epoch);
    let hour = secs_of_day / 3600;
    let min = (secs_of_day % 3600) / 60;
    let sec = secs_of_day % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{min:02}:{sec:02}Z")
}

/// Howard Hinnant's `civil_from_days` algorithm (public domain), converting
/// a day count since the Unix epoch into a proleptic Gregorian (y, m, d).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn after_time_formats_known_timestamp() {
        // 2024-01-01T00:00:00Z = 1704067200
        assert_eq!(chrono_after_time(1_704_067_200), "2024-01-01T00:00:00Z");
    }

    #[test]
    fn classify_contract_error_body_maps_known_reasons() {
        let client = SealedChainClient::new(1, "https://algod.example", "https://indexer.example");
        assert!(matches!(client.classify_contract_error_body("TAKEN"), ChainError::NameTaken));
        assert!(matches!(client.classify_contract_error_body("NO_CREDITS"), ChainError::NoCredits));
        assert!(matches!(client.classify_contract_error_body("BAD_CODE"), ChainError::BadRedeemCode));
        assert!(matches!(client.classify_contract_error_body("overspend"), ChainError::MbrShortfall));
        assert!(matches!(
            client.classify_contract_error_body("BAD_CHAR"),
            ChainError::BadUsernameFormat { code: "BAD_CHAR", .. }
        ));
        assert!(matches!(client.classify_contract_error_body("something else"), ChainError::Generic(_)));
    }

    fn sample_profile(encryption_pubkey: [u8; 32], scan_pubkey: [u8; 32], pq_pubkey_hash: Option<[u8; 32]>) -> UserProfile {
        UserProfile {
            wallet_address: "WALLET1".to_string(),
            username: None,
            encryption_pubkey,
            scan_pubkey,
            pq_public_key: None,
            pq_pubkey_hash,
        }
    }

    #[test]
    fn keys_are_stale_when_pq_hash_never_published() {
        let profile = sample_profile([1u8; 32], [2u8; 32], None);
        assert!(SealedChainClient::keys_are_stale(&profile, &[1u8; 32], &[2u8; 32], &[3u8; 32]));
    }

    #[test]
    fn keys_are_stale_when_encryption_pubkey_differs() {
        let profile = sample_profile([9u8; 32], [2u8; 32], Some([3u8; 32]));
        assert!(SealedChainClient::keys_are_stale(&profile, &[1u8; 32], &[2u8; 32], &[3u8; 32]));
    }

    #[test]
    fn keys_are_not_stale_when_everything_matches() {
        let profile = sample_profile([1u8; 32], [2u8; 32], Some([3u8; 32]));
        assert!(!SealedChainClient::keys_are_stale(&profile, &[1u8; 32], &[2u8; 32], &[3u8; 32]));
    }

    /// Live smoke test against the real public AlgoNode MainNet endpoint —
    /// same one the mobile app uses by default (`constants.dart`'s
    /// `ALGO_ALGOD_URL`). Only checks that request/response parsing works
    /// against the real algod API; ignored by default since it needs
    /// network access. NOTE: hits real MainNet since the 2026-08-06 cutover
    /// (`crate::constants::ALGO_ALGOD_TARGET_URL` is now MainNet) — costs
    /// nothing to run (read-only), but be aware before running
    /// `cargo test -- --ignored`.
    #[tokio::test]
    #[ignore]
    async fn get_suggested_params_against_real_mainnet() {
        // Targets go through the real OHTTP gateway now (see `SealedChainClient::new`),
        // which only proxies to Nodely's own `*.4160.nodely.dev` hosts — not
        // arbitrary algod hosts — so this must use the target constants, not
        // the direct `algonode.cloud` endpoints.
        let client = SealedChainClient::new(
            crate::constants::SEALED_APP_ID,
            crate::constants::ALGO_ALGOD_TARGET_URL,
            crate::constants::ALGO_INDEXER_TARGET_URL,
        );
        let params = client.get_suggested_params().await.unwrap();
        assert!(params.first_valid > 0);
        assert!(!params.genesis_id.is_empty());
    }

    /// Live smoke test: a freshly-generated (never funded) address reads
    /// back a balance of exactly 0 rather than erroring — confirms
    /// request/response parsing against real algod without needing a
    /// funded wallet. NOTE: hits real MainNet since the 2026-08-06 cutover
    /// — read-only, but see the note on the sibling test above.
    #[tokio::test]
    #[ignore]
    async fn get_account_balance_against_real_mainnet() {
        let client = SealedChainClient::new(
            crate::constants::SEALED_APP_ID,
            crate::constants::ALGO_ALGOD_TARGET_URL,
            crate::constants::ALGO_INDEXER_TARGET_URL,
        );
        let wallet = super::super::wallet::AlgorandWallet::create();
        let balance = client.get_account_balance(&wallet.address).await.unwrap();
        assert_eq!(balance, 0);
    }
}
