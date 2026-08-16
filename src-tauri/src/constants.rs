//! App-wide constants, ported from `sealed_app/lib/core/constants.dart`
//! (chain/indexer endpoints and IDs only — UI-layout constants aren't
//! relevant to the Rust backend).
//!
//! **MainNet (2026-08-06 cutover).** Matches `sealed_app`'s production
//! default (`RuntimeEnv.defaults()` = mainnet) — unlike the Flutter client,
//! this desktop build has no runtime network toggle, so these constants are
//! the only network this binary ever talks to. TestNet values are kept in
//! comments for revert, not as live (and therefore dead-code-warning-
//! triggering) constants.

/// Unified Sealed contract AVM App ID (MainNet, post 2026-06-24 MiMC
/// cutover — supersedes 3604741332). TestNet: 762153589 (stale — last
/// confirmed working value in this client; `sealed_app`'s current TestNet
/// debug target has since moved to 765017002, see `runtime_env.dart`).
pub const SEALED_APP_ID: u64 = 3615523287;

// ---------------------------------------------------------------------------
// OHTTP — Algod / Algorand-indexer channel (Nodely), ported from
// `constants.dart`'s `OHTTP_GATEWAY_CONFIG_URL`/`OHTTP_RELAY_URL`/
// `OHTTP_BUNDLED_CONFIG_HEX`/`OHTTP_TARGET_RPC_URL`/`OHTTP_TARGET_INDEXER_URL`.
// Gateway/relay/bundled-config are shared across MainNet and TestNet in the
// Dart source — only the proxied target host differs.
// ---------------------------------------------------------------------------

/// Gateway publishes its HPKE public key config at this URL. Only used to
/// regenerate `OHTTP_BUNDLED_CONFIG_HEX` after a key rotation — the running
/// app never GETs this directly (that would leak the caller's IP to the
/// gateway on cold start).
pub const OHTTP_GATEWAY_CONFIG_URL: &str = "https://ohttp.nodely.io/ohttp-configs";
/// Relay forwards encrypted requests without seeing their content.
pub const OHTTP_RELAY_URL: &str = "https://relay.oblivious.network/great-apple-60";
/// Bundled bootstrap key config for the channel above. Regenerate via:
/// `curl -fsS https://ohttp.nodely.io/ohttp-configs | xxd -p | tr -d '\n'`
pub const OHTTP_BUNDLED_CONFIG_HEX: &str =
    "800020b7eeeb4e0d4751b1298eb45ccffd37e909a52368309f5499e1b19b8a9f9ac712000400010001";
/// Actual algod target reached *through* the gateway above — it only
/// proxies to Nodely's own `*.4160.nodely.dev` hosts, not arbitrary algod
/// hosts, so this is what every OHTTP-routed algod request must target
/// (there is no direct, non-OHTTP algod URL constant in this client).
/// TestNet: `https://testnet-api.4160.nodely.dev`.
pub const ALGO_ALGOD_TARGET_URL: &str = "https://mainnet-api.4160.nodely.dev";
/// Same, for the Algorand indexer (search/box-history reads).
/// TestNet: `https://testnet-idx.4160.nodely.dev`.
pub const ALGO_INDEXER_TARGET_URL: &str = "https://mainnet-idx.4160.nodely.dev";

// ---------------------------------------------------------------------------
// OHTTP — Sealed-indexer channel (VPS MainNet deployment). A separate
// gateway/relay from the algod channel above — do NOT reuse them, the
// gateway's `TARGET_REWRITES` pins to a specific upstream (see
// `infra/vps/RUNBOOK.md`). Values mirror `constants.dart`'s (unsuffixed,
// i.e. production-default) `INDEXER_OHTTP_GATEWAY_CONFIG_URL`/`_RELAY_URL`/
// `_BUNDLED_CONFIG_HEX`. TestNet has its own separate deployment at
// `gw-testnet.sealed.channel` (`constants.dart`'s `*_TESTNET` siblings) —
// not used by this client since the cutover.
// ---------------------------------------------------------------------------

/// Sealed indexer service base URL — also the OHTTP target (the gateway
/// rewrites this same host to the real indexer backend; path is preserved).
/// TestNet: `https://gw-testnet.sealed.channel`.
pub const INDEXER_BASE_URL: &str = "https://gw.sealed.channel";
pub const INDEXER_OHTTP_GATEWAY_CONFIG_URL: &str = "https://gw.sealed.channel/ohttp-configs";
pub const INDEXER_OHTTP_RELAY_URL: &str = "https://relay.oblivious.network/alter-ball-33";
/// Regenerate via: `curl -fsS https://gw.sealed.channel/ohttp-configs | xxd -p | tr -d '\n'`
pub const INDEXER_OHTTP_BUNDLED_CONFIG_HEX: &str =
    "8100209cf2b505fb3b20028d2c2b1177b3c3057165e5fed848822b46ad635234ad7004000400010001";
