//! ChatGPT account actions for the desktop process.
//!
//! The web view asks for these over Tauri IPC. [`ChatgptAccount`] in
//! `workshop-gateway` (reached through `workshop-server-api`) reads the sidecar
//! owner proof and calls the Gateway over loopback HTTP, while this module
//! enforces the window capability gate, opens the sign-in URL natively in the
//! system browser, and projects outcomes onto [`AccountView`] and
//! [`EnableModelsReply`]. Values returned to IPC never carry an owner proof, an
//! API bearer, a capability token, or the authorize URL.
//!
//! # Trust boundary: capability gate and owner proof
//!
//! The embedded Gateway config frame runs on its own dedicated loopback
//! origin and is excluded from Workshop's main-origin Tauri remote ACL, while
//! [`capability`] adds per-load capability defense in depth on every account
//! command. Account commands therefore sit behind two separate boundaries:
//!
//! - **Page to desktop ([`capability`]):** The top-level main document
//!   binds a fresh capability token once at load through
//!   [`chatgpt_bind_capability`]. Every other account command checks that
//!   token before touching disk or the Gateway. A second bind on the same
//!   load is refused, and [`ChatgptState::reset_for_main_load`] resets the
//!   gate only when navigation verifies a top-level main-origin load.
//! - **Desktop to Gateway ([`ChatgptAccount`]):** `workshop-gateway` reads
//!   both the `gateway.json` API bearer and the `gateway-private.json`
//!   `owner_proof` header for the matching `(pid, epoch)` boot identity. Those
//!   secrets remain strictly inside `workshop-gateway` and never enter the
//!   desktop view layer or cross IPC.
//!
//! Sign-in launches the external browser natively via `tauri_plugin_opener`
//! inside [`chatgpt_sign_in`] and discards both the authorize URL and any
//! raw OS opener error afterwards so the web view never receives the OAuth
//! URL or URL-bearing error text.

mod capability;
mod view;

use workshop_server_api::ChatgptAccount;

#[cfg(test)]
pub(crate) use capability::CapabilityGate;
pub(crate) use capability::ChatgptState;
use capability::admit;
pub(crate) use view::{AccountView, EnableModelsReply};
#[cfg(test)]
pub(crate) use view::{RenamedModel, SkippedModel, WAITING_DETAIL, enable_reply_from_body};
use view::{enable_outcome, refuse_view, view_of_account_error, view_of_status};
#[cfg(test)]
use view::{ipc_json, ipc_json_leaks};

async fn status_view_for(account: &ChatgptAccount) -> AccountView {
    match account.status().await {
        Ok(body) => view_of_status(&body),
        Err(error) => view_of_account_error(error),
    }
}

async fn sign_in_view_for<F, E>(account: &ChatgptAccount, open: F) -> AccountView
where
    F: FnOnce(&str) -> Result<(), E>,
{
    match account.sign_in(open).await {
        Ok(body) => view_of_status(&body),
        Err(error) => view_of_account_error(error),
    }
}

async fn sign_out_view_for(account: &ChatgptAccount) -> AccountView {
    match account.sign_out().await {
        Ok(body) => view_of_status(&body),
        Err(error) => view_of_account_error(error),
    }
}

async fn reset_view_for(account: &ChatgptAccount) -> AccountView {
    match account.reset().await {
        Ok(body) => view_of_status(&body),
        Err(error) => view_of_account_error(error),
    }
}

async fn enable_models_for(account: &ChatgptAccount) -> Result<EnableModelsReply, String> {
    let (status, body) = account
        .enable_models()
        .await
        .map_err(|error| view_of_account_error(error).detail)?;
    enable_outcome(status, &body)
}

/// Binds the per-window-load capability token sent by the top-level main
/// frame. Once bound, subsequent calls fail until the main origin reloads.
///
/// # Errors
/// Returns an error if `capability` is empty or a capability is already
/// bound for the current main-document load.
#[tauri::command]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Tauri deserializes command arguments by value"
)]
pub(crate) fn chatgpt_bind_capability(
    state: tauri::State<'_, ChatgptState>,
    capability: String,
) -> Result<(), String> {
    state.bind(&capability)
}

/// Fetches the sanitized ChatGPT account status from the running Gateway.
///
/// # Errors
/// Returns an error when the presented `capability` does not match the
/// bound token. Missing run folders or Gateway errors return an
/// unavailable/unreadable [`AccountView`] instead of exposing raw secrets.
#[tauri::command]
pub(crate) async fn chatgpt_status(
    state: tauri::State<'_, ChatgptState>,
    capability: String,
) -> Result<AccountView, String> {
    admit(&state, &capability)?;
    Ok(match ChatgptAccount::from_default_run_dir() {
        Some(account) => status_view_for(&account).await,
        None => refuse_view(),
    })
}

/// Starts a ChatGPT OAuth sign-in flow and opens the authorize URL natively
/// in the system browser. The authorize URL never crosses back into the web
/// view, and any OS opener error is replaced with a fixed status sentence.
///
/// # Errors
/// Returns an error when the presented `capability` does not match the
/// bound token.
#[tauri::command]
pub(crate) async fn chatgpt_sign_in(
    app: tauri::AppHandle,
    state: tauri::State<'_, ChatgptState>,
    capability: String,
) -> Result<AccountView, String> {
    admit(&state, &capability)?;
    let Some(account) = ChatgptAccount::from_default_run_dir() else {
        return Ok(refuse_view());
    };
    Ok(sign_in_view_for(&account, |url| {
        tauri_plugin_opener::OpenerExt::opener(&app).open_url(url, None::<&str>)
    })
    .await)
}

/// Signs out the active ChatGPT session via the Gateway and returns the
/// refreshed account status.
///
/// # Errors
/// Returns an error when the presented `capability` does not match the
/// bound token.
#[tauri::command]
pub(crate) async fn chatgpt_sign_out(
    state: tauri::State<'_, ChatgptState>,
    capability: String,
) -> Result<AccountView, String> {
    admit(&state, &capability)?;
    Ok(match ChatgptAccount::from_default_run_dir() {
        Some(account) => sign_out_view_for(&account).await,
        None => refuse_view(),
    })
}

/// Resets persisted ChatGPT sign-in state via the Gateway and returns the
/// refreshed account status.
///
/// # Errors
/// Returns an error when the presented `capability` does not match the
/// bound token.
#[tauri::command]
pub(crate) async fn chatgpt_reset(
    state: tauri::State<'_, ChatgptState>,
    capability: String,
) -> Result<AccountView, String> {
    admit(&state, &capability)?;
    Ok(match ChatgptAccount::from_default_run_dir() {
        Some(account) => reset_view_for(&account).await,
        None => refuse_view(),
    })
}

/// Requests enabling ChatGPT plan models on the Gateway, returning only the
/// sanitized model outcome lists.
///
/// # Errors
/// Returns an error if the presented `capability` is invalid, no user run
/// folder resolves, Gateway discovery/transport fails, or the Gateway
/// rejects the request (mapped to bounded error codes such as
/// `config_has_pending_changes`, `owner_proof_required`, or `gateway_error`).
#[tauri::command]
pub(crate) async fn chatgpt_enable(
    state: tauri::State<'_, ChatgptState>,
    capability: String,
) -> Result<EnableModelsReply, String> {
    admit(&state, &capability)?;
    let Some(account) = ChatgptAccount::from_default_run_dir() else {
        return Err(
            "ChatGPT model enable requires a resolved user run folder; none is configured"
                .to_owned(),
        );
    };
    enable_models_for(&account).await
}

#[cfg(test)]
#[path = "chatgpt-tests.rs"]
mod tests;
