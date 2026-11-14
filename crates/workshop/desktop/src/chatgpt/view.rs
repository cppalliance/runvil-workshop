//! IPC view types and Gateway response projections for ChatGPT accounts.
//!
//! Only [`AccountView`] and [`EnableModelsReply`] cross back to the web
//! view. Parsing and projection here drop sensitive fields (the API bearer,
//! owner proof, `authorize_url`, and arbitrary Gateway error prose that may
//! name filesystem paths or pending draft details).

use serde::{Deserialize, Serialize};
use workshop_server_api::{
    ChatgptAccountError, ChatgptAccountOp, ChatgptHttpFailure, ChatgptJsonFailure,
    ChatgptReadTarget, ChatgptSidecarFailure,
};

/// Shown while a sign-in is waiting on the browser.
pub(crate) const WAITING_DETAIL: &str = "Waiting for the browser. Quitting Workshop stops the Gateway, and the browser page will then fail.";

/// What the web view is allowed to know about the account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AccountView {
    /// `signed_out`, `waiting`, `signed_in`, `needs_sign_in`,
    /// `unavailable`, or `unreadable`.
    pub state: String,
    /// The signed-in account's email, when the Gateway has one.
    pub email: Option<String>,
    /// The account granted plan usage.
    pub plan_granted: bool,
    /// A short sentence for the status element.
    pub detail: String,
    /// Caller-facing plan model ids, in catalog order.
    #[serde(default)]
    pub plan_models: Vec<String>,
    /// The Gateway's own status word: `active`, `signed_out`, `needs_auth`.
    #[serde(default)]
    pub account_status: Option<String>,
    /// `confirmed` or `unconfirmed` for the last remote sign-out.
    #[serde(default)]
    pub revocation: Option<String>,
    /// A token the Gateway did not keep may still be live at the issuer.
    #[serde(default)]
    pub cleanup_unconfirmed: bool,
    /// Why sign-in cannot be held, when `state` is `unavailable`.
    #[serde(default)]
    pub unavailable_reason: Option<String>,
}

/// One model the Gateway renamed because the original id was taken.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct RenamedModel {
    pub from: String,
    pub to: String,
}

/// One model the Gateway did not add, with a closed reason.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SkippedModel {
    pub name: String,
    pub reason: String,
}

/// The reply of `POST /admin/chatgpt/models/enable`.
///
/// Shapes match the Gateway contract: `added` and `renamed` are the
/// caller-facing ids, and `skipped` is `{name, reason}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct EnableModelsReply {
    #[serde(default)]
    pub endpoint: String,
    #[serde(default)]
    pub added: Vec<String>,
    #[serde(default)]
    pub renamed: Vec<RenamedModel>,
    #[serde(default)]
    pub skipped: Vec<SkippedModel>,
}

/// The Gateway's status body. Extra fields are ignored.
#[derive(Debug, Deserialize)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "each flag is a field of the Gateway status body"
)]
struct GatewayStatus {
    status: Option<String>,
    #[serde(default)]
    unreadable: bool,
    email: Option<String>,
    #[serde(default)]
    plan_granted: bool,
    #[serde(default)]
    sign_in_pending: bool,
    #[serde(default = "yes")]
    available: bool,
    unavailable_reason: Option<String>,
    #[serde(default)]
    plan_models: Vec<String>,
    #[serde(default)]
    revocation: Option<String>,
    #[serde(default)]
    cleanup_unconfirmed: bool,
}

fn yes() -> bool {
    true
}

fn json_failure_summary(failure: ChatgptJsonFailure) -> String {
    let (kind, line, column) = match failure {
        ChatgptJsonFailure::Io { line, column } => ("JSON I/O error", line, column),
        ChatgptJsonFailure::Syntax { line, column } => ("JSON syntax error", line, column),
        ChatgptJsonFailure::Data { line, column } => ("JSON schema mismatch", line, column),
        ChatgptJsonFailure::Eof { line, column } => ("unexpected end of JSON", line, column),
    };
    if line > 0 {
        format!("{kind} at line {line}, column {column}")
    } else {
        kind.to_owned()
    }
}

fn sidecar_failure_summary(failure: ChatgptSidecarFailure) -> String {
    match failure {
        ChatgptSidecarFailure::Read => "file I/O read failure".to_owned(),
        ChatgptSidecarFailure::Parse(json) => {
            format!("malformed JSON ({})", json_failure_summary(json))
        }
        ChatgptSidecarFailure::Invalid => "invalid sidecar field".to_owned(),
        ChatgptSidecarFailure::Other => "sidecar file error".to_owned(),
    }
}

const fn op_method_and_path(op: ChatgptAccountOp) -> (&'static str, &'static str) {
    match op {
        ChatgptAccountOp::Status => ("GET", "/admin/chatgpt/status"),
        ChatgptAccountOp::SignIn => ("POST", "/admin/chatgpt/sign-in"),
        ChatgptAccountOp::SignOut => ("POST", "/admin/chatgpt/sign-out"),
        ChatgptAccountOp::Reset => ("POST", "/admin/chatgpt/reset"),
        ChatgptAccountOp::EnableModels => ("POST", "/admin/chatgpt/models/enable"),
    }
}

const fn read_target_noun(target: ChatgptReadTarget) -> &'static str {
    match target {
        ChatgptReadTarget::Status => "status",
        ChatgptReadTarget::SignIn => "sign-in",
        ChatgptReadTarget::EnableModels => "enable",
    }
}

fn http_failure_kind(failure: ChatgptHttpFailure) -> &'static str {
    match failure {
        ChatgptHttpFailure::Timeout => "request timed out",
        ChatgptHttpFailure::Connect => "connection error",
        ChatgptHttpFailure::Decode => "response decode error",
        ChatgptHttpFailure::Body => "body transfer error",
        ChatgptHttpFailure::Redirect => "redirect blocked",
        ChatgptHttpFailure::Builder => "client builder error",
        ChatgptHttpFailure::Request => "request build error",
        ChatgptHttpFailure::Transport => "transport error",
    }
}

fn op_request_detail(op: ChatgptAccountOp, kind: ChatgptHttpFailure) -> String {
    let (method, path) = op_method_and_path(op);
    format!(
        "Gateway {method} {path} request failed: {}",
        http_failure_kind(kind)
    )
}

fn op_status_detail(op: ChatgptAccountOp, status: u16) -> String {
    let (method, path) = op_method_and_path(op);
    format!("Gateway {method} {path} returned HTTP {status} (required 2xx success)")
}

/// Maps a closed [`ChatgptAccountError`] from `workshop-gateway` onto the
/// fixed-sentence [`AccountView`] returned over Tauri IPC.
#[must_use]
pub(super) fn view_of_account_error(error: ChatgptAccountError) -> AccountView {
    let (state, detail) = match error {
        ChatgptAccountError::DiscoveryMissing => (
            "unavailable",
            "Local Gateway unavailable: required discovery file gateway.json, none found"
                .to_owned(),
        ),
        ChatgptAccountError::DiscoveryUnreadable(failure) => (
            "unavailable",
            format!(
                "Local Gateway discovery file could not be read: {}",
                sidecar_failure_summary(failure)
            ),
        ),
        ChatgptAccountError::OwnerProofMissing => (
            "unreadable",
            "Gateway owner-proof file is missing: required private sidecar matching the active Gateway, found none"
                .to_owned(),
        ),
        ChatgptAccountError::OwnerProofUnreadable(failure) => (
            "unreadable",
            format!(
                "Gateway owner-proof file could not be read: {}",
                sidecar_failure_summary(failure)
            ),
        ),
        ChatgptAccountError::BootMismatch {
            discovery_pid,
            discovery_epoch,
            private_pid,
            private_epoch,
        } => (
            "unreadable",
            format!(
                "Gateway owner-proof file belongs to another Gateway boot: required matching (pid, epoch) ({discovery_pid}, {discovery_epoch}), found ({private_pid}, {private_epoch})"
            ),
        ),
        ChatgptAccountError::ClientBuild(kind) => (
            "unavailable",
            format!(
                "Loopback HTTP account client failed to build: {}",
                http_failure_kind(kind)
            ),
        ),
        ChatgptAccountError::Request { op, kind } => ("unavailable", op_request_detail(op, kind)),
        ChatgptAccountError::ReadBody {
            target,
            status,
            kind,
        } => (
            "unavailable",
            format!(
                "Reading Gateway {} reply body failed (HTTP {status}): {}",
                read_target_noun(target),
                http_failure_kind(kind)
            ),
        ),
        ChatgptAccountError::HttpStatus { op, status } => {
            ("unavailable", op_status_detail(op, status))
        }
        ChatgptAccountError::StatusInvalidJson(json) => (
            "unreadable",
            format!(
                "Gateway status reply is not valid JSON: required JSON object, found {}",
                json_failure_summary(json)
            ),
        ),
        ChatgptAccountError::SignInInvalidJson(json) => (
            "unavailable",
            format!(
                "Gateway sign-in reply is not valid JSON: required JSON object with sign-in launch URL, found {}",
                json_failure_summary(json)
            ),
        ),
        ChatgptAccountError::SignInMissingUrl => (
            "unavailable",
            "Gateway sign-in reply is missing a non-empty sign-in launch URL string".to_owned(),
        ),
        ChatgptAccountError::BrowserOpen => (
            "unavailable",
            "Opening the ChatGPT sign-in page in the system browser failed: required a working system URL handler, found opener error"
                .to_owned(),
        ),
    };
    view(state, None, false, detail, Vec::new())
}

/// Maps a Gateway status body onto the view the web view may hold.
#[must_use]
pub(crate) fn view_of_status(status: &serde_json::Value) -> AccountView {
    let parsed: GatewayStatus = match serde_json::from_value(status.clone()) {
        Ok(parsed) => parsed,
        Err(error) => {
            return view(
                "unreadable",
                None,
                false,
                format!(
                    "Gateway status payload is unreadable: required valid Gateway status JSON, found {}",
                    json_failure_summary(ChatgptJsonFailure::from_serde(&error))
                ),
                Vec::new(),
            );
        }
    };
    let plans = parsed.plan_models.clone();
    let mut shown = if !parsed.available {
        view(
            "unavailable",
            None,
            false,
            parsed.unavailable_reason.clone().unwrap_or_else(|| {
                "Gateway reported ChatGPT sign-in unavailable with no reason given".to_owned()
            }),
            Vec::new(),
        )
    } else if parsed.unreadable {
        view(
            "unreadable",
            None,
            false,
            "Gateway reported saved ChatGPT sign-in record is unreadable (required valid persisted sign-in data)",
            Vec::new(),
        )
    } else if parsed.sign_in_pending {
        view(
            "waiting",
            parsed.email.clone(),
            parsed.plan_granted,
            WAITING_DETAIL,
            plans,
        )
    } else if parsed.status.as_deref() == Some("active") && parsed.plan_granted {
        view("signed_in", parsed.email.clone(), true, "Signed in", plans)
    } else if parsed.status.as_deref() == Some("needs_auth")
        || parsed.status.as_deref() == Some("active")
    {
        let detail = if parsed.status.as_deref() == Some("active") {
            "ChatGPT account is active without plan access; sign in with an account that has plan access".to_owned()
        } else {
            "ChatGPT session needs sign-in again (required active credentials, current status is needs_auth)".to_owned()
        };
        view(
            "needs_sign_in",
            parsed.email.clone(),
            parsed.plan_granted,
            detail,
            plans,
        )
    } else {
        view(
            "signed_out",
            parsed.email.clone(),
            false,
            "Signed out",
            plans,
        )
    };
    shown.account_status = parsed.status;
    shown.revocation = parsed.revocation;
    shown.cleanup_unconfirmed = parsed.cleanup_unconfirmed;
    shown.unavailable_reason = parsed.unavailable_reason;
    shown
}

/// Builds an [`AccountView`] with optional status metadata cleared.
fn view(
    state: &str,
    email: Option<String>,
    plan_granted: bool,
    detail: impl Into<String>,
    plan_models: Vec<String>,
) -> AccountView {
    AccountView {
        state: state.to_owned(),
        email,
        plan_granted,
        detail: detail.into(),
        plan_models,
        account_status: None,
        revocation: None,
        cleanup_unconfirmed: false,
        unavailable_reason: None,
    }
}

/// Unavailable status view returned when no writable user run folder resolves.
pub(super) fn refuse_view() -> AccountView {
    view(
        "unavailable",
        None,
        false,
        "ChatGPT account action requires a resolved user run folder; none is configured",
        Vec::new(),
    )
}

/// The enable reply, keeping only the three lists.
///
/// # Errors
/// Returns an error when the body is not that JSON object.
#[cfg(test)]
pub(crate) fn enable_reply_from_body(body: &str) -> Result<EnableModelsReply, String> {
    serde_json::from_str(body).map_err(|error| {
        format!(
            "Gateway enable reply is not valid JSON: required EnableModelsReply JSON object, found {}",
            json_failure_summary(ChatgptJsonFailure::from_serde(&error))
        )
    })
}

/// A successful enable list, or the Gateway's error code.
///
/// A non-2xx body is not a reply. The code is the only text returned, so a
/// message that names a path or a draft does not reach the web view.
/// `config_has_pending_changes` stays recognizable.
///
/// # Errors
/// Returns a bounded Gateway error code (`error.code` when short ASCII
/// identifier, otherwise `"gateway_error"`) when `status` is non-2xx, `body`
/// is unreadable, or the JSON body carries an error envelope.
pub(crate) fn enable_outcome(status: u16, body: &str) -> Result<EnableModelsReply, String> {
    if !(200..300).contains(&status) {
        return Err(enable_error_code(body));
    }
    let value: serde_json::Value =
        serde_json::from_str(body).map_err(|_| "gateway_error".to_owned())?;
    if value.get("added").is_none() || value.get("error").is_some() {
        return Err(enable_error_code(body));
    }
    serde_json::from_value(value).map_err(|_| "gateway_error".to_owned())
}

/// The `error.code` string, when it is a short token. Otherwise a fixed word.
const ENABLE_CODE_LIMIT: usize = 64;

fn enable_error_code(body: &str) -> String {
    let code = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|value| {
            value
                .get("error")
                .and_then(|error| error.get("code"))
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        });
    match code {
        Some(code)
            if !code.is_empty()
                && code.chars().count() <= ENABLE_CODE_LIMIT
                && code.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') =>
        {
            code
        }
        _ => "gateway_error".to_owned(),
    }
}

/// JSON for an IPC payload. The caller checks it carries no secret.
#[must_use]
#[cfg(test)]
pub(super) fn ipc_json<T: Serialize>(value: &T) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "{}".to_owned())
}

/// Whether `json` names a secret the web view must not receive.
#[must_use]
#[cfg(test)]
pub(super) fn ipc_json_leaks(json: &str) -> bool {
    const NEEDLES: &[&str] = &[
        "authorize_url",
        "authorizeUrl",
        "owner_proof",
        "ownerProof",
        "plan_proof",
        "planProof",
        "api_key",
        "apiKey",
        "access_token",
        "accessToken",
        "refresh_token",
        "refreshToken",
    ];
    NEEDLES.iter().any(|needle| json.contains(needle))
}
