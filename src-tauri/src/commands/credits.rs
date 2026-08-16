//! Credits commands: balance read, cost estimate, redeem. Mirrors
//! `services/credits_service.dart`.

use tauri::State;

use crate::credits::preimage_from_code;
use crate::state::AppState;

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreditCostInfo {
    pub current: u64,
    pub after: u64,
}

/// Current credit balance for the active session's wallet. 0 for unfunded
/// accounts (no `w:` box yet).
#[tauri::command]
pub async fn get_credits(state: State<'_, AppState>) -> Result<u64, String> {
    let session_guard = state.session.lock().await;
    let session = session_guard.as_ref().ok_or("not unlocked")?;
    session
        .chain_client
        .get_credits(&session.wallet, &session.wallet.address)
        .await
        .map_err(|e| e.to_string())
}

/// Dry-run credit cost for the next 1-credit action (claim/release/publish
/// keys). Errors with `NoCredits` (as its string form) if the balance is 0.
#[tauri::command]
pub async fn estimate_credit_cost(state: State<'_, AppState>) -> Result<CreditCostInfo, String> {
    let session_guard = state.session.lock().await;
    let session = session_guard.as_ref().ok_or("not unlocked")?;
    let cost = session.chain_client.estimate_credit_cost(&session.wallet).await.map_err(|e| e.to_string())?;
    Ok(CreditCostInfo { current: cost.current, after: cost.after })
}

/// Redeem a code (format: `XXXX-XXXX-XXXX-XXXX`, dashes/case/spacing don't
/// matter — see `crate::credits::normalize_code`). `username`, if non-empty,
/// claims it in the same group.
#[tauri::command]
pub async fn redeem_code(state: State<'_, AppState>, code: String, username: Option<String>) -> Result<String, String> {
    let preimage = preimage_from_code(&code).map_err(|e| e.to_string())?;
    let username = username.unwrap_or_default();

    let tx_id = {
        let session_guard = state.session.lock().await;
        let session = session_guard.as_ref().ok_or("not unlocked")?;
        session
            .chain_client
            .redeem(&session.wallet, &session.escrow, &preimage, &username)
            .await
            .map_err(|e| e.to_string())?
    };

    // Best-effort: this redeem is the first point a `w:` box + credit
    // definitely exist, so it's the natural place to opportunistically
    // publish this session's keys on-chain if they aren't already. Must not
    // fail the redeem itself — a missed publish here just gets retried on
    // the next unlock or redeem.
    let _ = crate::commands::keys::ensure_keys_published_impl(&state).await;

    Ok(tx_id)
}
