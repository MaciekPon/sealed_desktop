//! Oblivious HTTP (RFC 9458) client, ported from `sealed_app/lib/remote/ohttp/*.dart`.
//! Anonymizes RPC requests: the relay sees the caller's IP but not the
//! request content; the gateway sees the request content but not the IP.

pub mod binary_http;
pub mod client;
pub mod config;
pub mod hpke;
