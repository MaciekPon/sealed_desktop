//! Algorand chain integration: wallet, canonical msgpack transaction
//! encoding, the TreasuryEscrow LogicSig signer, group/txn-id computation,
//! and the network-calling `SealedChainClient` (algod/indexer HTTP, box
//! reads, ARC4 decoding, contract error mapping). Ported from
//! `sealed_app/lib/chain/*.dart`.

pub mod address;
pub mod client;
pub mod escrow;
pub mod msgpack;
pub mod txn;
pub mod user_state;
pub mod wallet;
