//! Anonymous-redeem building blocks (SPEC-onchain-mimc-tornado). Phase 6 of
//! the desktop parity roadmap: ports the MiMC/BN254 hash + Merkle tree
//! primitives from `sealed_app/lib/infra/zk/` as a standalone, testable
//! module. Deliberately **not** wired into any Tauri command or the redeem
//! flow yet — witness generation, SNARK proving, and on-chain wiring are
//! future work (see the plan file's Phase 6 scope note).
//!
//! `dead_code` is allowed crate-wide for this module tree: every public
//! item here is exercised by its own golden-vector tests but, by design,
//! has no caller yet outside `#[cfg(test)]` — that's the point of shipping
//! the foundation before the thing that consumes it.
#![allow(dead_code)]

pub mod mimc;
pub mod mimc_tree;
