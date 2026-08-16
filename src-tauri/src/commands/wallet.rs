//! Wallet commands: balance read. Mirrors the relevant slice of
//! `WalletService` (`refreshBalance`).

use tauri::State;

use crate::state::AppState;

/// Current on-chain balance for the active session's wallet, in microAlgos.
#[tauri::command]
pub async fn get_wallet_balance(state: State<'_, AppState>) -> Result<u64, String> {
    let session_guard = state.session.lock().await;
    let session = session_guard.as_ref().ok_or("not unlocked")?;
    session.chain_client.get_account_balance(&session.wallet.address).await.map_err(|e| e.to_string())
}
