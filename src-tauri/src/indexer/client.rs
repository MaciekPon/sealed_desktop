//! Client for the Sealed indexer service, ported from
//! `remote/indexer_client.dart`.
//!
//! Public surface: username search, username availability, PQ pubkey
//! fetch. Push-token registration (`registerPushToken`/`unregisterPushToken`
//! in the Dart source) is intentionally **not** ported — replaced by the
//! background-poll + native-toast design (see the plan), so nothing calls
//! the indexer's push endpoints from this client.
//!
//! Every request routes through [`OhttpHttpClient`] — a separate gateway/relay
//! from the algod/Algorand-indexer channel (`chain::client::SealedChainClient`
//! uses its own); do not conflate them, see `constants::INDEXER_OHTTP_*` vs
//! `constants::OHTTP_*`.

use base64::Engine;
use thiserror::Error;
use url::Url;

use crate::ohttp::client::{OhttpError, OhttpHttpClient, OhttpResponse};

#[derive(Debug, Error)]
pub enum IndexerError {
    #[error("query must be non-empty")]
    EmptyQuery,
    #[error("name must be 3-20 chars matching ^[a-z][a-z0-9_]*[a-z0-9]$")]
    InvalidUsernameFormat,
    #[error("walletAddress must be a 58-char Algorand base32 address")]
    InvalidWalletAddress,
    #[error("not published")]
    NotPublished,
    #[error("indexer error ({status:?}): {message}")]
    Api { status: Option<u16>, message: String },
    #[error("ohttp error: {0}")]
    Ohttp(#[from] OhttpError),
    #[error("unexpected response shape: {0}")]
    UnexpectedResponse(String),
}

#[derive(Debug)]
pub struct UsernameSearchHit {
    pub username: String,
    pub wallet_address: String,
}

#[derive(Debug)]
pub struct UsernameSearchResult {
    pub query: String,
    pub count: u64,
    pub users: Vec<UsernameSearchHit>,
}

#[derive(Debug)]
pub struct PqPublicKeyResponse {
    pub pq_pubkey: Vec<u8>,
    pub pq_pubkey_hash: [u8; 32],
    pub published_at_round: i64,
}

pub struct IndexerClient {
    base_url: String,
    ohttp: OhttpHttpClient,
}

impl IndexerClient {
    pub fn new(base_url: impl AsRef<str>) -> Self {
        Self {
            base_url: normalize_base_url(base_url.as_ref()),
            ohttp: OhttpHttpClient::new_with_bundled_config(
                crate::constants::INDEXER_OHTTP_GATEWAY_CONFIG_URL,
                crate::constants::INDEXER_OHTTP_RELAY_URL,
                crate::constants::INDEXER_OHTTP_BUNDLED_CONFIG_HEX,
            ),
        }
    }

    /// Fuzzy username search — `GET /username/search?q=&limit=`.
    pub async fn search_users(&self, query: &str, limit: u32) -> Result<UsernameSearchResult, IndexerError> {
        let trimmed = query.trim();
        if trimmed.is_empty() {
            return Err(IndexerError::EmptyQuery);
        }
        let clamped_limit = limit.clamp(1, 50);

        let mut url =
            Url::parse(&format!("{}/username/search", self.base_url)).map_err(|e| IndexerError::UnexpectedResponse(e.to_string()))?;
        url.query_pairs_mut().append_pair("q", trimmed).append_pair("limit", &clamped_limit.to_string());

        let resp = self.ohttp.get(&url, &[]).await?;
        if !resp.is_success() {
            return Err(self.api_error(&resp));
        }
        let body: serde_json::Value =
            serde_json::from_slice(&resp.body).map_err(|e| IndexerError::UnexpectedResponse(e.to_string()))?;
        let users = body
            .get("results")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|hit| {
                        Some(UsernameSearchHit {
                            username: hit.get("username")?.as_str()?.to_string(),
                            wallet_address: hit.get("wallet")?.as_str()?.to_string(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();

        Ok(UsernameSearchResult {
            query: body.get("query").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            count: body.get("count").and_then(|v| v.as_u64()).unwrap_or(0),
            users,
        })
    }

    /// Check availability of a username — `GET /username/:name/available`.
    pub async fn check_username_available(&self, name: &str) -> Result<bool, IndexerError> {
        let lower = name.to_lowercase();
        if lower.len() < 3 || lower.len() > 20 || !is_valid_username_shape(&lower) {
            return Err(IndexerError::InvalidUsernameFormat);
        }

        let url = Url::parse(&format!("{}/username/{}/available", self.base_url, urlencoding_component(&lower)))
            .map_err(|e| IndexerError::UnexpectedResponse(e.to_string()))?;

        let resp = self.ohttp.get(&url, &[]).await?;
        if !resp.is_success() {
            return Err(self.api_error(&resp));
        }
        let body: serde_json::Value =
            serde_json::from_slice(&resp.body).map_err(|e| IndexerError::UnexpectedResponse(e.to_string()))?;
        Ok(body.get("available").and_then(|v| v.as_bool()).unwrap_or(false))
    }

    /// Fetch the user's PQ public key and on-chain hash —
    /// `GET /user/:wallet/pq-pubkey`. `NotPublished` on 404.
    pub async fn get_pq_public_key(&self, wallet_address: &str) -> Result<PqPublicKeyResponse, IndexerError> {
        if wallet_address.len() != 58 {
            return Err(IndexerError::InvalidWalletAddress);
        }

        let url = Url::parse(&format!("{}/user/{}/pq-pubkey", self.base_url, urlencoding_component(wallet_address)))
            .map_err(|e| IndexerError::UnexpectedResponse(e.to_string()))?;

        let resp = self.ohttp.get(&url, &[]).await?;
        if resp.status_code == 404 {
            return Err(IndexerError::NotPublished);
        }
        if !resp.is_success() {
            return Err(self.api_error(&resp));
        }

        let body: serde_json::Value =
            serde_json::from_slice(&resp.body).map_err(|e| IndexerError::UnexpectedResponse(e.to_string()))?;
        let pq_pubkey_b64 = body
            .get("pqPubkey")
            .and_then(|v| v.as_str())
            .ok_or_else(|| IndexerError::UnexpectedResponse("missing pqPubkey".into()))?;
        let pq_pubkey = base64::engine::general_purpose::STANDARD
            .decode(pq_pubkey_b64)
            .map_err(|e| IndexerError::UnexpectedResponse(e.to_string()))?;
        let hash_hex = body
            .get("pqPubkeyHash")
            .and_then(|v| v.as_str())
            .ok_or_else(|| IndexerError::UnexpectedResponse("missing pqPubkeyHash".into()))?;
        let pq_pubkey_hash = decode_hex_32(hash_hex)?;
        let published_at_round = body
            .get("publishedAtRound")
            .and_then(|v| v.as_i64())
            .ok_or_else(|| IndexerError::UnexpectedResponse("missing publishedAtRound".into()))?;

        Ok(PqPublicKeyResponse {
            pq_pubkey,
            pq_pubkey_hash,
            published_at_round,
        })
    }

    fn api_error(&self, resp: &OhttpResponse) -> IndexerError {
        let message = serde_json::from_slice::<serde_json::Value>(&resp.body)
            .ok()
            .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(str::to_string))
            .unwrap_or_else(|| "request failed".to_string());
        IndexerError::Api { status: Some(resp.status_code as u16), message }
    }
}

fn normalize_base_url(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        trimmed.to_string()
    } else {
        format!("https://{trimmed}")
    }
}

fn is_valid_username_shape(name: &str) -> bool {
    let bytes = name.as_bytes();
    let Some(&first) = bytes.first() else { return false };
    let Some(&last) = bytes.last() else { return false };
    if !first.is_ascii_lowercase() {
        return false;
    }
    if !(last.is_ascii_lowercase() || last.is_ascii_digit()) {
        return false;
    }
    bytes.iter().all(|&b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

fn urlencoding_component(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

fn decode_hex_32(hex: &str) -> Result<[u8; 32], IndexerError> {
    if hex.len() != 64 {
        return Err(IndexerError::UnexpectedResponse(format!("expected 64 hex chars, got {}", hex.len())));
    }
    let mut out = [0u8; 32];
    for i in 0..32 {
        out[i] = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16)
            .map_err(|_| IndexerError::UnexpectedResponse("invalid hex".into()))?;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_bare_host_to_https() {
        assert_eq!(normalize_base_url("indexer.example.com"), "https://indexer.example.com");
        assert_eq!(normalize_base_url("https://indexer.example.com"), "https://indexer.example.com");
        assert_eq!(normalize_base_url("http://localhost:8080"), "http://localhost:8080");
    }

    #[test]
    fn username_shape_validation() {
        assert!(is_valid_username_shape("alice"));
        assert!(is_valid_username_shape("a1_bc"));
        assert!(!is_valid_username_shape("_alice"));
        assert!(!is_valid_username_shape("1alice"));
        assert!(!is_valid_username_shape("alice_"));
        assert!(!is_valid_username_shape("Alice"));
    }

    #[test]
    fn hex32_round_trip() {
        let hex = "aa".repeat(32);
        let decoded = decode_hex_32(&hex).unwrap();
        assert_eq!(decoded, [0xAAu8; 32]);
    }

    #[tokio::test]
    async fn empty_query_is_rejected() {
        let client = IndexerClient::new("https://indexer.example.com");
        let err = client.search_users("   ", 20).await.unwrap_err();
        assert!(matches!(err, IndexerError::EmptyQuery));
    }

    #[tokio::test]
    async fn invalid_username_format_is_rejected() {
        let client = IndexerClient::new("https://indexer.example.com");
        assert!(matches!(
            client.check_username_available("_bad").await.unwrap_err(),
            IndexerError::InvalidUsernameFormat
        ));
        assert!(matches!(
            client.check_username_available("ab").await.unwrap_err(),
            IndexerError::InvalidUsernameFormat
        ));
    }

    #[tokio::test]
    async fn invalid_wallet_address_length_is_rejected() {
        let client = IndexerClient::new("https://indexer.example.com");
        assert!(matches!(
            client.get_pq_public_key("tooshort").await.unwrap_err(),
            IndexerError::InvalidWalletAddress
        ));
    }
}
