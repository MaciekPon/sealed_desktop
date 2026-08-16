//! Tauri-managed application state: the unlocked session (vault, DB,
//! wallet, derived keys, chain/indexer clients) tying together every
//! module built so far. Mirrors what `service_locator.dart` +
//! `app_providers.dart` wire up via Riverpod on mobile, minus the
//! not-yet-ported domain services (message/user/alias-chat/credits).

use std::path::PathBuf;

use tokio::sync::Mutex;

use crate::chain::client::SealedChainClient;
use crate::chain::escrow::{build_treasury_escrow_signer, TreasuryEscrowSigner};
use crate::chain::wallet::AlgorandWallet;
use crate::constants;
use crate::db::Db;
use crate::dek::Vault;
use crate::indexer::client::IndexerClient;
use crate::keys::SealedKeys;

/// Everything needed to serve requests while the vault is unlocked.
pub struct Session {
    pub vault: Vault,
    pub dek: [u8; 32],
    pub db: Db,
    pub wallet: AlgorandWallet,
    pub sealed_keys: SealedKeys,
    pub escrow: TreasuryEscrowSigner,
    pub chain_client: SealedChainClient,
    pub indexer_client: IndexerClient,
}

impl Session {
    pub fn new(vault: Vault, dek: [u8; 32], db: Db, wallet: AlgorandWallet, sealed_keys: SealedKeys) -> Self {
        Self {
            vault,
            dek,
            db,
            wallet,
            sealed_keys,
            escrow: build_treasury_escrow_signer(),
            chain_client: SealedChainClient::new(
                constants::SEALED_APP_ID,
                constants::ALGO_ALGOD_TARGET_URL,
                constants::ALGO_INDEXER_TARGET_URL,
            ),
            indexer_client: IndexerClient::new(constants::INDEXER_BASE_URL),
        }
    }
}

pub struct AppState {
    pub session: Mutex<Option<Session>>,
    pub app_dir: PathBuf,
}

impl AppState {
    pub fn new(app_dir: PathBuf) -> Self {
        Self {
            session: Mutex::new(None),
            app_dir,
        }
    }

    pub fn db_path(&self) -> PathBuf {
        self.app_dir.join("sealed_messages.db")
    }
}
