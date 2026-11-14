//! Loopback HTTP calls to the local Gateway's `/admin/chatgpt/*` endpoints.
//!
//! Every request authenticates with two secrets from `run_dir`: the `api_key`
//! bearer from `gateway.json` and the `owner_proof` header from
//! `gateway-private.json`. [`boot_secrets`] verifies via
//! [`GatewayPrivateFile::names_boot_of`] that both files came from the same
//! `(pid, epoch)` Gateway boot so stale secrets from a previous process are
//! rejected before any request leaves this process.
//!
//! Admin requests use [`http_client`], which turns off HTTP redirects and
//! system proxies so bearer and owner-proof headers stay strictly on loopback
//! `127.0.0.1`. Sign-in passes the OAuth launch URL only to the caller's native
//! opener closure and discards both the URL and any raw opener error afterwards.
//!
//! [`ChatgptAccount::status`], [`ChatgptAccount::sign_in`],
//! [`ChatgptAccount::sign_out`], and [`ChatgptAccount::reset`] return the
//! Gateway's status JSON as received, and [`ChatgptAccount::enable_models`]
//! returns the HTTP status and body as received; the desktop app projects both
//! before anything reaches the page. Every failure from this module surfaces as
//! a `Copy` [`ChatgptAccountError`] composed solely of closed enum tags and
//! numeric identifiers, carrying no external text, paths, or secrets.

use std::fmt;
use std::path::{Path, PathBuf};

use workshop_gateway_discovery::{
    GatewayDiscoveryFile, GatewayPrivateFile, SidecarError, default_run_dir,
};

/// Header the Gateway's owner check reads on `/admin/chatgpt/*`. Not a bearer.
const OWNER_HEADER: &str = "x-runvil-owner-proof";

/// Closed `/admin/chatgpt/*` operation identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatgptAccountOp {
    /// `GET /admin/chatgpt/status`.
    Status,
    /// `POST /admin/chatgpt/sign-in`.
    SignIn,
    /// `POST /admin/chatgpt/sign-out`.
    SignOut,
    /// `POST /admin/chatgpt/reset`.
    Reset,
    /// `POST /admin/chatgpt/models/enable`.
    EnableModels,
}

impl ChatgptAccountOp {
    const fn request_parts(self) -> (reqwest::Method, &'static str) {
        match self {
            Self::Status => (reqwest::Method::GET, "/admin/chatgpt/status"),
            Self::SignIn => (reqwest::Method::POST, "/admin/chatgpt/sign-in"),
            Self::SignOut => (reqwest::Method::POST, "/admin/chatgpt/sign-out"),
            Self::Reset => (reqwest::Method::POST, "/admin/chatgpt/reset"),
            Self::EnableModels => (reqwest::Method::POST, "/admin/chatgpt/models/enable"),
        }
    }
}

/// Closed response-body read target for `/admin/chatgpt/*` calls that read a body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatgptReadTarget {
    /// `GET /admin/chatgpt/status` reply body.
    Status,
    /// `POST /admin/chatgpt/sign-in` reply body.
    SignIn,
    /// `POST /admin/chatgpt/models/enable` reply body.
    EnableModels,
}

/// Closed `serde_json` failure classification without offending value text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatgptJsonFailure {
    /// Underlying reader or writer I/O error.
    Io {
        /// 1-based line number, or `0` when unavailable.
        line: usize,
        /// 1-based column number, or `0` when unavailable.
        column: usize,
    },
    /// Malformed JSON syntax.
    Syntax {
        /// 1-based line number, or `0` when unavailable.
        line: usize,
        /// 1-based column number, or `0` when unavailable.
        column: usize,
    },
    /// Valid JSON syntax whose types or fields did not match the target shape.
    Data {
        /// 1-based line number, or `0` when unavailable.
        line: usize,
        /// 1-based column number, or `0` when unavailable.
        column: usize,
    },
    /// Input ended before a complete JSON value was read.
    Eof {
        /// 1-based line number, or `0` when unavailable.
        line: usize,
        /// 1-based column number, or `0` when unavailable.
        column: usize,
    },
}

impl ChatgptJsonFailure {
    /// Classifies `error` using only its category and line/column coordinates,
    /// never invoking `Display` (which can echo offending string values).
    #[must_use]
    pub fn from_serde(error: &serde_json::Error) -> Self {
        let line = error.line();
        let column = error.column();
        match error.classify() {
            serde_json::error::Category::Io => Self::Io { line, column },
            serde_json::error::Category::Syntax => Self::Syntax { line, column },
            serde_json::error::Category::Data => Self::Data { line, column },
            serde_json::error::Category::Eof => Self::Eof { line, column },
        }
    }
}

/// Closed [`SidecarError`] classification for sidecar file reads without
/// filesystem paths or raw OS/JSON strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatgptSidecarFailure {
    /// Sidecar file existed but could not be read.
    Read,
    /// Sidecar file was not valid JSON for its schema.
    Parse(ChatgptJsonFailure),
    /// Sidecar file failed structural validation.
    Invalid,
    /// Any other `SidecarError` variant.
    Other,
}

impl ChatgptSidecarFailure {
    fn from_sidecar(error: &SidecarError) -> Self {
        match error {
            SidecarError::Read { .. } => Self::Read,
            SidecarError::Parse { source, .. } => {
                Self::Parse(ChatgptJsonFailure::from_serde(source))
            }
            SidecarError::Invalid { .. } => Self::Invalid,
            _ => Self::Other,
        }
    }
}

/// Closed [`reqwest::Error`] classification without URLs or external `Display` text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatgptHttpFailure {
    /// Request or body read timed out.
    Timeout,
    /// TCP loopback connection failed.
    Connect,
    /// Response body decoding failed.
    Decode,
    /// Request or response body transfer failed.
    Body,
    /// Redirect policy blocked a redirect.
    Redirect,
    /// HTTP client builder failed to initialize.
    Builder,
    /// Outbound HTTP request failed to construct.
    Request,
    /// Other transport failure.
    Transport,
}

impl ChatgptHttpFailure {
    fn from_reqwest(error: &reqwest::Error) -> Self {
        if error.is_timeout() {
            Self::Timeout
        } else if error.is_connect() {
            Self::Connect
        } else if error.is_decode() {
            Self::Decode
        } else if error.is_body() {
            Self::Body
        } else if error.is_redirect() {
            Self::Redirect
        } else if error.is_builder() {
            Self::Builder
        } else if error.is_request() {
            Self::Request
        } else {
            Self::Transport
        }
    }
}

/// Closed, `Copy` failure enum for `/admin/chatgpt/*` operations.
///
/// Contains only closed enum categories and integer status/boot/coordinate
/// fields, so no external text, secret, URL, or filesystem path can be carried
/// in an error value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatgptAccountError {
    /// `gateway.json` was absent from `run_dir`.
    DiscoveryMissing,
    /// `gateway.json` could not be read or validated.
    DiscoveryUnreadable(ChatgptSidecarFailure),
    /// `gateway-private.json` was absent from `run_dir`.
    OwnerProofMissing,
    /// `gateway-private.json` could not be read or validated.
    OwnerProofUnreadable(ChatgptSidecarFailure),
    /// `gateway-private.json` named a different `(pid, epoch)` boot than `gateway.json`.
    BootMismatch {
        /// Active `gateway.json` process ID.
        discovery_pid: u32,
        /// Active `gateway.json` boot epoch.
        discovery_epoch: u64,
        /// Stale `gateway-private.json` process ID.
        private_pid: u32,
        /// Stale `gateway-private.json` boot epoch.
        private_epoch: u64,
    },
    /// Loopback HTTP client failed to initialize.
    ClientBuild(ChatgptHttpFailure),
    /// Sending the loopback HTTP request failed.
    Request {
        /// Closed admin operation identifier.
        op: ChatgptAccountOp,
        /// Closed transport failure category.
        kind: ChatgptHttpFailure,
    },
    /// Reading the HTTP response body failed.
    ReadBody {
        /// Closed response-body target identifier.
        target: ChatgptReadTarget,
        /// HTTP status code received before the body failed.
        status: u16,
        /// Closed transport failure category.
        kind: ChatgptHttpFailure,
    },
    /// Endpoint returned a non-2xx HTTP status where 2xx was required.
    HttpStatus {
        /// Closed admin operation identifier.
        op: ChatgptAccountOp,
        /// Non-2xx HTTP status code.
        status: u16,
    },
    /// `GET /admin/chatgpt/status` body was not valid JSON.
    StatusInvalidJson(ChatgptJsonFailure),
    /// `POST /admin/chatgpt/sign-in` body was not valid JSON.
    SignInInvalidJson(ChatgptJsonFailure),
    /// `POST /admin/chatgpt/sign-in` JSON omitted a non-empty `authorize_url` string.
    SignInMissingUrl,
    /// Native system browser opener returned an error.
    BrowserOpen,
}

/// Process-local loopback base URL, API bearer, and owner proof for one call.
struct BootSecrets {
    base_url: String,
    api_key: String,
    owner_proof: String,
}

impl fmt::Debug for BootSecrets {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BootSecrets")
            .field("base_url", &self.base_url)
            .field("api_key", &"<redacted>")
            .field("owner_proof", &"<redacted>")
            .finish()
    }
}

fn boot_secrets(run_dir: &Path) -> Result<BootSecrets, ChatgptAccountError> {
    let discovery = match GatewayDiscoveryFile::read(run_dir) {
        Ok(Some(discovery)) => discovery,
        Ok(None) => return Err(ChatgptAccountError::DiscoveryMissing),
        Err(error) => {
            return Err(ChatgptAccountError::DiscoveryUnreadable(
                ChatgptSidecarFailure::from_sidecar(&error),
            ));
        }
    };
    let private = match GatewayPrivateFile::read(run_dir) {
        Ok(Some(private)) => private,
        Ok(None) => return Err(ChatgptAccountError::OwnerProofMissing),
        Err(error) => {
            return Err(ChatgptAccountError::OwnerProofUnreadable(
                ChatgptSidecarFailure::from_sidecar(&error),
            ));
        }
    };
    if !private.names_boot_of(&discovery) {
        return Err(ChatgptAccountError::BootMismatch {
            discovery_pid: discovery.pid,
            discovery_epoch: discovery.epoch,
            private_pid: private.pid,
            private_epoch: private.epoch,
        });
    }
    Ok(BootSecrets {
        base_url: format!("http://127.0.0.1:{}", discovery.port),
        api_key: discovery.api_key,
        owner_proof: private.owner_proof,
    })
}

fn http_client() -> Result<reqwest::Client, ChatgptAccountError> {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .build()
        .map_err(|error| ChatgptAccountError::ClientBuild(ChatgptHttpFailure::from_reqwest(&error)))
}

async fn gateway_call(
    op: ChatgptAccountOp,
    secrets: &BootSecrets,
) -> Result<reqwest::Response, ChatgptAccountError> {
    let (method, path) = op.request_parts();
    http_client()?
        .request(method, format!("{}{path}", secrets.base_url))
        .bearer_auth(&secrets.api_key)
        .header(OWNER_HEADER, &secrets.owner_proof)
        .send()
        .await
        .map_err(|error| ChatgptAccountError::Request {
            op,
            kind: ChatgptHttpFailure::from_reqwest(&error),
        })
}

/// Extracts the `authorize_url` from a sign-in reply body for the native
/// browser opener only.
fn authorize_url_from_body(body: &str) -> Result<String, ChatgptAccountError> {
    let value: serde_json::Value = serde_json::from_str(body).map_err(|error| {
        ChatgptAccountError::SignInInvalidJson(ChatgptJsonFailure::from_serde(&error))
    })?;
    value
        .get("authorize_url")
        .and_then(serde_json::Value::as_str)
        .filter(|url| !url.is_empty())
        .map(str::to_owned)
        .ok_or(ChatgptAccountError::SignInMissingUrl)
}

/// Loopback caller for the local Gateway's `/admin/chatgpt/*` endpoints.
///
/// Reads `gateway.json` and `gateway-private.json` from `run_dir` on each
/// operation, verifies matching `(pid, epoch)` boot identity, and keeps both
/// `api_key` and `owner_proof` strictly inside `workshop-gateway`. Successful
/// calls return the Gateway's status JSON or `(status, body)` pair as received;
/// the desktop app projects those replies before anything reaches the page.
#[derive(Debug, Clone)]
pub struct ChatgptAccount {
    run_dir: PathBuf,
}

impl ChatgptAccount {
    /// Takes an explicit run directory.
    #[must_use]
    pub fn new(run_dir: impl Into<PathBuf>) -> Self {
        Self {
            run_dir: run_dir.into(),
        }
    }

    /// Binds account operations to the platform default Gateway `run_dir`,
    /// when a user run directory resolves.
    #[must_use]
    pub fn from_default_run_dir() -> Option<Self> {
        default_run_dir().map(Self::new)
    }

    /// Calls `GET /admin/chatgpt/status` and returns the Gateway's status JSON
    /// as received, for the desktop app to project before anything reaches the
    /// page.
    ///
    /// # Errors
    /// Returns [`ChatgptAccountError`] if sidecar secrets are missing or
    /// mismatched, transport fails, the Gateway answers non-2xx, or the body
    /// is not valid JSON.
    pub async fn status(&self) -> Result<serde_json::Value, ChatgptAccountError> {
        let secrets = boot_secrets(&self.run_dir)?;
        let op = ChatgptAccountOp::Status;
        let response = gateway_call(op, &secrets).await?;
        let status = response.status();
        let status_code = status.as_u16();
        let body_text = response
            .text()
            .await
            .map_err(|error| ChatgptAccountError::ReadBody {
                target: ChatgptReadTarget::Status,
                status: status_code,
                kind: ChatgptHttpFailure::from_reqwest(&error),
            })?;
        if !status.is_success() {
            return Err(ChatgptAccountError::HttpStatus {
                op,
                status: status_code,
            });
        }
        serde_json::from_str(&body_text).map_err(|error| {
            ChatgptAccountError::StatusInvalidJson(ChatgptJsonFailure::from_serde(&error))
        })
    }

    /// Starts an OAuth sign-in via `POST /admin/chatgpt/sign-in`.
    ///
    /// The URL parsed from the sign-in reply is passed to `open` and used for
    /// nothing else, and an error from `open` is discarded. The status JSON
    /// that follows is returned as received, and the desktop app projects it.
    ///
    /// # Errors
    /// Returns [`ChatgptAccountError`] if sidecar verification, HTTP transport,
    /// sign-in payload parsing, browser launch, or the follow-up status read
    /// fails.
    pub async fn sign_in<F, E>(&self, open: F) -> Result<serde_json::Value, ChatgptAccountError>
    where
        F: FnOnce(&str) -> Result<(), E>,
    {
        let secrets = boot_secrets(&self.run_dir)?;
        let op = ChatgptAccountOp::SignIn;
        let response = gateway_call(op, &secrets).await?;
        let status = response.status();
        let status_code = status.as_u16();
        let body = response
            .text()
            .await
            .map_err(|error| ChatgptAccountError::ReadBody {
                target: ChatgptReadTarget::SignIn,
                status: status_code,
                kind: ChatgptHttpFailure::from_reqwest(&error),
            })?;
        if !status.is_success() {
            return Err(ChatgptAccountError::HttpStatus {
                op,
                status: status_code,
            });
        }
        let url = authorize_url_from_body(&body)?;
        if open(&url).is_err() {
            return Err(ChatgptAccountError::BrowserOpen);
        }
        self.status().await
    }

    /// Signs out the active ChatGPT session via `POST /admin/chatgpt/sign-out`
    /// and returns the Gateway's refreshed status JSON as received, for the
    /// desktop app to project before anything reaches the page.
    ///
    /// # Errors
    /// Returns [`ChatgptAccountError`] if sidecar verification, transport,
    /// non-2xx response status, or the follow-up status read fails.
    pub async fn sign_out(&self) -> Result<serde_json::Value, ChatgptAccountError> {
        self.post_then_status(ChatgptAccountOp::SignOut).await
    }

    /// Resets persisted ChatGPT sign-in state via `POST /admin/chatgpt/reset`
    /// and returns the Gateway's refreshed status JSON as received, for the
    /// desktop app to project before anything reaches the page.
    ///
    /// # Errors
    /// Returns [`ChatgptAccountError`] if sidecar verification, transport,
    /// non-2xx response status, or the follow-up status read fails.
    pub async fn reset(&self) -> Result<serde_json::Value, ChatgptAccountError> {
        self.post_then_status(ChatgptAccountOp::Reset).await
    }

    /// Calls `POST /admin/chatgpt/models/enable` and returns the HTTP status
    /// and body as received, for the desktop app to project before anything
    /// reaches the page.
    ///
    /// # Errors
    /// Returns [`ChatgptAccountError`] if sidecar verification, request
    /// dispatch, or reading the response body fails.
    pub async fn enable_models(&self) -> Result<(u16, String), ChatgptAccountError> {
        let secrets = boot_secrets(&self.run_dir)?;
        let response = gateway_call(ChatgptAccountOp::EnableModels, &secrets).await?;
        let status = response.status().as_u16();
        let body = response
            .text()
            .await
            .map_err(|error| ChatgptAccountError::ReadBody {
                target: ChatgptReadTarget::EnableModels,
                status,
                kind: ChatgptHttpFailure::from_reqwest(&error),
            })?;
        Ok((status, body))
    }

    async fn post_then_status(
        &self,
        op: ChatgptAccountOp,
    ) -> Result<serde_json::Value, ChatgptAccountError> {
        let secrets = boot_secrets(&self.run_dir)?;
        let response = gateway_call(op, &secrets).await?;
        let status = response.status();
        if !status.is_success() {
            return Err(ChatgptAccountError::HttpStatus {
                op,
                status: status.as_u16(),
            });
        }
        self.status().await
    }
}

#[cfg(test)]
#[path = "account-tests.rs"]
mod tests;
