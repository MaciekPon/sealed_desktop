//! Alias-chat: a local QR/paste key-exchange handshake between two ephemeral
//! X25519+ML-KEM-512 identities, established once and then reused to send
//! messages through the exact same `sendMessage` chain call normal
//! wallet-to-wallet DMs use. See the plan's "Faza 7f" section for the full
//! design (ported from `sealed_app/lib/features/messaging/alias/`).
//!
//! There is no on-chain "channel" concept — `createChannel`/`acceptChannel`/
//! `readChannel`/`deleteChannel` don't exist in the deployed contract (they
//! were added then removed from `contract.algo.ts` in the same commit).
//! Everything here is local key exchange plus reuse of the existing
//! `chain::client::SealedChainClient::send_message`.

pub mod contacts;
pub mod envelope;
pub mod incoming_invites;
pub mod invite_delivery;
pub mod messages;
pub mod messaging;
pub mod onboarding;

#[derive(Debug, thiserror::Error)]
pub enum AliasError {
    #[error("malformed invite envelope")]
    MalformedInviteEnvelope,
    #[error("malformed accept envelope")]
    MalformedAcceptEnvelope,
    #[error("no matching pending invite for this reply")]
    NoMatchingPendingInvite,
    #[error("crypto error: {0}")]
    Crypto(#[from] crate::crypto::CryptoError),
    #[error("db error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("chain error: {0}")]
    Chain(#[from] crate::chain::client::ChainError),
    #[error("messaging error: {0}")]
    Messaging(#[from] crate::messaging::MessagingError),
    #[error("no credits available. Redeem a code to send messages.")]
    NoCredits,
}
