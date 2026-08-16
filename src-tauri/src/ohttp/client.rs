//! HTTP client that routes requests through OHTTP for IP privacy, ported
//! from `remote/ohttp/ohttp_http_client.dart`.
//!
//! The relay sees the caller's IP but not the request content; the gateway
//! sees the request content but not the caller's IP.

use std::sync::RwLock;
use std::time::{Duration, Instant};

use thiserror::Error;
use url::Url;

use super::config::{OhttpConfig, OhttpConfigError};
use super::hpke::{self, HpkeError};

#[derive(Debug, Error)]
pub enum OhttpError {
    #[error("failed to fetch OHTTP config: {0}")]
    ConfigFetch(String),
    #[error("config parse error: {0}")]
    Config(#[from] OhttpConfigError),
    #[error("hpke error: {0}")]
    Hpke(#[from] HpkeError),
    #[error("binary http error: {0}")]
    BinaryHttp(#[from] super::binary_http::BinaryHttpError),
    #[error("relay error: {0}")]
    Relay(String),
    #[error("relay returned non-encapsulated body (content-type={content_type}, len={len})")]
    NonEncapsulatedBody { content_type: String, len: usize },
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
}

pub struct OhttpResponse {
    pub status_code: u32,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl OhttpResponse {
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status_code)
    }
}

const CONFIG_CACHE_DURATION: Duration = Duration::from_secs(30 * 60);

pub struct OhttpHttpClient {
    gateway_config_url: String,
    relay_url: String,
    http: reqwest::Client,
    /// Set only by [`Self::new_with_bundled_config`]. When present, used
    /// forever in place of a live fetch — see [`Self::get_config`].
    bundled_config: Option<OhttpConfig>,
    cached_config: RwLock<Option<(OhttpConfig, Instant)>>,
}

impl OhttpHttpClient {
    pub fn new(gateway_config_url: impl Into<String>, relay_url: impl Into<String>) -> Self {
        Self {
            gateway_config_url: gateway_config_url.into(),
            relay_url: relay_url.into(),
            http: reqwest::Client::new(),
            bundled_config: None,
            cached_config: RwLock::new(None),
        }
    }

    /// Like [`Self::new`], but seeded with a bundled bootstrap key config
    /// (hex-encoded RFC 9458 key config, e.g. from `curl <gateway>/ohttp-configs
    /// | xxd -p | tr -d '\n'`) so the very first request never does a
    /// plaintext GET to `gateway_config_url` — that GET would itself reveal
    /// the caller's IP to the gateway, defeating OHTTP's IP-blindness on
    /// cold start. Mirrors `OHTTP_BUNDLED_CONFIG_HEX` /
    /// `INDEXER_OHTTP_BUNDLED_CONFIG_HEX` in `constants.dart`.
    ///
    /// The bundled config is used for the client's entire lifetime — no
    /// periodic re-fetch is attempted. A gateway key rotation requires
    /// shipping a new bundled hex (same tradeoff the Dart source accepts:
    /// see `infra/vps/RUNBOOK.md`'s key-rotation procedure).
    ///
    /// # Panics
    /// If `bundled_config_hex` isn't valid hex, or doesn't parse as an RFC
    /// 9458 key config — both are build-time invariants of our own
    /// hardcoded constants, verified by this module's tests.
    pub fn new_with_bundled_config(gateway_config_url: impl Into<String>, relay_url: impl Into<String>, bundled_config_hex: &str) -> Self {
        let bytes = decode_hex(bundled_config_hex).expect("bundled OHTTP config must be valid hex");
        let config = OhttpConfig::from_bytes(&bytes).expect("bundled OHTTP config bytes must parse as an RFC 9458 key config");
        Self {
            gateway_config_url: gateway_config_url.into(),
            relay_url: relay_url.into(),
            http: reqwest::Client::new(),
            bundled_config: Some(config),
            cached_config: RwLock::new(None),
        }
    }

    pub async fn get(&self, url: &Url, headers: &[(String, String)]) -> Result<OhttpResponse, OhttpError> {
        self.request("GET", url, headers, None).await
    }

    pub async fn post(&self, url: &Url, headers: &[(String, String)], body: Option<&[u8]>) -> Result<OhttpResponse, OhttpError> {
        self.request("POST", url, headers, body).await
    }

    /// Fetch and cache the gateway's OHTTP key configuration. Returns the
    /// bundled config immediately (no network call at all) if this client
    /// was built via [`Self::new_with_bundled_config`].
    pub async fn get_config(&self) -> Result<OhttpConfig, OhttpError> {
        if let Some(bundled) = &self.bundled_config {
            return Ok(bundled.clone());
        }

        if let Some((cached, fetched_at)) = self.cached_config.read().unwrap().as_ref() {
            if fetched_at.elapsed() < CONFIG_CACHE_DURATION {
                return Ok(OhttpConfig {
                    key_id: cached.key_id,
                    kem_id: cached.kem_id,
                    kdf_id: cached.kdf_id,
                    aead_id: cached.aead_id,
                    public_key: cached.public_key.clone(),
                });
            }
        }

        let resp = self.http.get(&self.gateway_config_url).send().await?;
        if !resp.status().is_success() {
            return Err(OhttpError::ConfigFetch(resp.status().to_string()));
        }
        let bytes = resp.bytes().await?;
        let config = OhttpConfig::from_bytes(&bytes)?;

        let cached_copy = OhttpConfig {
            key_id: config.key_id,
            kem_id: config.kem_id,
            kdf_id: config.kdf_id,
            aead_id: config.aead_id,
            public_key: config.public_key.clone(),
        };
        *self.cached_config.write().unwrap() = Some((cached_copy, Instant::now()));

        Ok(config)
    }

    async fn request(
        &self,
        method: &str,
        url: &Url,
        headers: &[(String, String)],
        body: Option<&[u8]>,
    ) -> Result<OhttpResponse, OhttpError> {
        let config = self.get_config().await?;

        let binary_request = super::binary_http::encode_request(method, url, headers, body)?;
        let encapsulated = hpke::encapsulate_request(&config, &binary_request)?;

        let relay_response = self
            .http
            .post(&self.relay_url)
            .header("Content-Type", "message/ohttp-req")
            .header("Accept", "message/ohttp-res")
            .body(encapsulated.encapsulated_message)
            .timeout(Duration::from_secs(20))
            .send()
            .await?;

        if !relay_response.status().is_success() {
            return Err(OhttpError::Relay(relay_response.status().to_string()));
        }

        let content_type = relay_response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_lowercase();
        if !content_type.contains("message/ohttp-res") {
            let body_bytes = relay_response.bytes().await.unwrap_or_default();
            return Err(OhttpError::NonEncapsulatedBody { content_type, len: body_bytes.len() });
        }

        let response_bytes = relay_response.bytes().await?;
        let binary_response = hpke::decapsulate_response(&response_bytes, &encapsulated.enc, &encapsulated.secret)?;

        Ok(OhttpResponse {
            status_code: binary_response.status_code,
            headers: binary_response.headers,
            body: binary_response.body,
        })
    }
}

fn decode_hex(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_hex_round_trips() {
        assert_eq!(decode_hex("00ab"), Some(vec![0x00, 0xAB]));
        assert_eq!(decode_hex(""), Some(vec![]));
        assert_eq!(decode_hex("abc"), None); // odd length
        assert_eq!(decode_hex("zz"), None); // not hex
    }

    /// Build-time guard: our hardcoded bundled configs must actually parse
    /// as valid RFC 9458 key configs — this is what `new_with_bundled_config`
    /// otherwise only discovers via a runtime panic.
    #[test]
    fn bundled_ohttp_configs_parse() {
        for hex in [
            crate::constants::OHTTP_BUNDLED_CONFIG_HEX,
            crate::constants::INDEXER_OHTTP_BUNDLED_CONFIG_HEX,
        ] {
            let bytes = decode_hex(hex).unwrap_or_else(|| panic!("not valid hex: {hex}"));
            OhttpConfig::from_bytes(&bytes).unwrap_or_else(|e| panic!("failed to parse bundled config {hex}: {e}"));
        }
    }

    #[tokio::test]
    async fn new_with_bundled_config_never_needs_a_live_fetch() {
        let client = OhttpHttpClient::new_with_bundled_config(
            "https://example.invalid/ohttp-configs",
            "https://example.invalid/relay",
            crate::constants::OHTTP_BUNDLED_CONFIG_HEX,
        );
        // No network access happens in this test — `get_config` must return
        // the bundled config directly without ever touching `self.http`.
        let config = client.get_config().await.unwrap();
        assert_eq!(config.kem_id, 0x0020);
    }
}
