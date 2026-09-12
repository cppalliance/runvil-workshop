//! The gateway client's wire types: the buffered relay response, the
//! forwarded config-panel response, the SSE payload stream, and the
//! typed cache and profile-switch events decoded from it.

use std::path::PathBuf;
use std::pin::Pin;

use futures_util::stream::{Stream, StreamExt};
use serde::Deserialize;

use super::GatewayError;

/// A gateway HTTP response captured for verbatim relay.
#[derive(Debug)]
pub struct GatewayResponse {
    /// The gateway's status code, relayed unchanged.
    pub status: reqwest::StatusCode,
    /// The gateway's response body, relayed byte-for-byte.
    pub body: Vec<u8>,
}

/// A gateway response captured for the config-panel proxy: the relay
/// keeps the content type alongside the status and body, because the
/// config UI distinguishes a buffered JSON answer from an SSE stream by
/// it.
#[derive(Debug)]
pub struct ForwardedResponse {
    /// The gateway's status code, relayed unchanged.
    pub status: reqwest::StatusCode,
    /// The gateway's `Content-Type`, when it sent one.
    pub content_type: Option<String>,
    /// The gateway's response body, relayed byte-for-byte.
    pub body: Vec<u8>,
}

/// A stream of SSE `data:` payloads from the gateway, in arrival order.
///
/// Each item is one event's data, verbatim. A transport failure mid-stream
/// yields one error item and then ends the stream.
pub type SsePayloadStream = Pin<Box<dyn Stream<Item = Result<String, GatewayError>> + Send>>;

/// The gateway's answer to a cache-ensure request, `POST /v1/cache`.
///
/// The gateway answers a cache hit with a buffered JSON `ready` event and a
/// miss with an SSE stream of `downloading` progress events terminated by a
/// `ready` or `error` event; both event shapes decode as [`CacheEvent`]. A
/// non-success status (a declined or failed request) is buffered rather
/// than reported as an error, matching the relay contract of the other
/// client methods.
#[non_exhaustive]
pub enum CacheResponse {
    /// The gateway is downloading the blob; `payloads` carries the SSE
    /// stream of [`CacheEvent`] JSON documents.
    Download {
        /// The gateway's success status.
        status: reqwest::StatusCode,
        /// The SSE payload stream, ending in a terminal `ready` or `error`
        /// event.
        payloads: SsePayloadStream,
    },

    /// Any other answer, buffered: a cache hit's `ready` JSON on a success
    /// status, or the gateway's error envelope on a failure status.
    Buffered(GatewayResponse),
}

// Manual because the boxed payload stream has no `Debug` impl.
impl std::fmt::Debug for CacheResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Download { status, .. } => f
                .debug_struct("CacheResponse::Download")
                .field("status", status)
                .finish_non_exhaustive(),
            Self::Buffered(response) => f
                .debug_tuple("CacheResponse::Buffered")
                .field(response)
                .finish(),
        }
    }
}

/// One event of the gateway cache API: a download progress sample, or the
/// terminal state of a cache-ensure call.
///
/// The `path` a `Ready` event carries names a file on the gateway host, so
/// the cache API is only meaningful to a workshop sharing the gateway's
/// filesystem - the standard local deployment, where both run on loopback.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "status", rename_all = "lowercase")]
#[non_exhaustive]
pub enum CacheEvent {
    /// A progress sample from a running download.
    Downloading {
        /// Cumulative bytes downloaded so far.
        bytes: u64,
        /// Total bytes expected; null when the upstream server sent no
        /// Content-Length.
        total: Option<u64>,
    },

    /// The blob is cached and ready at `path`.
    Ready {
        /// Local path of the cached blob on the gateway host.
        path: PathBuf,
    },

    /// The download failed.
    Error {
        /// The gateway's description of the failure.
        message: String,
    },
}

/// The gateway's answer to a profile switch, `POST /admin/switch-profile`.
///
/// An accepted switch answers `text/event-stream`: stage markers as the
/// switch proceeds, then exactly one terminal `ready` or `error` event, all
/// decoding as [`SwitchEvent`]. A refusal before the switch starts (bad
/// auth, a malformed name, no profiles directory) is buffered rather than
/// reported as an error, matching the relay contract of the other client
/// methods.
#[non_exhaustive]
pub enum SwitchResponse {
    /// The gateway accepted the switch and is streaming its progress.
    Switching {
        /// The gateway's success status.
        status: reqwest::StatusCode,
        /// The SSE payload stream of [`SwitchEvent`] JSON documents, ending
        /// in a terminal `ready` or `error` event.
        payloads: SsePayloadStream,
    },

    /// A refusal, buffered: the gateway's error envelope.
    Buffered(GatewayResponse),
}

// Manual because the boxed payload stream has no `Debug` impl.
impl std::fmt::Debug for SwitchResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Switching { status, .. } => f
                .debug_struct("SwitchResponse::Switching")
                .field("status", status)
                .finish_non_exhaustive(),
            Self::Buffered(response) => f
                .debug_tuple("SwitchResponse::Buffered")
                .field(response)
                .finish(),
        }
    }
}

/// One event of the gateway's switch-profile stream: a stage marker as the
/// switch proceeds, then exactly one terminal event.
///
/// Stage markers arrive in execution order - `loading-profile`,
/// `stopping-models`, `starting-models` (the long pole: weights loading
/// into VRAM). The stage stays a string so a gateway that grows a new
/// stage never breaks the decode.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(untagged)]
#[non_exhaustive]
pub enum SwitchEvent {
    /// A phase of the switch is beginning.
    Stage {
        /// The gateway's name for the phase, e.g. `starting-models`.
        stage: String,
    },

    /// Terminal: the switch committed and `profile` is live.
    Ready {
        /// The now-active profile name.
        profile: String,
    },

    /// Terminal: the switch failed. The previous profile stays
    /// authenticated and remote-routable, but its local children may
    /// already be gone (the gateway's documented degraded state).
    Error {
        /// The gateway's description of the failure.
        message: String,
    },
}

/// A stream of decoded [`SwitchEvent`]s, as produced by [`switch_events`].
pub type SwitchEventStream = Pin<Box<dyn Stream<Item = Result<SwitchEvent, GatewayError>> + Send>>;

/// Decodes a switch-profile payload stream into typed [`SwitchEvent`]s.
///
/// A payload that does not parse as a switch event is logged and skipped -
/// a malformed line from the gateway degrades one progress update, never
/// the switch - and the stream continues to its terminal event. A transport
/// failure passes through and ends the stream.
#[must_use]
pub fn switch_events(payloads: SsePayloadStream) -> SwitchEventStream {
    Box::pin(payloads.filter_map(|item| async move {
        match item {
            Ok(payload) => match serde_json::from_str::<SwitchEvent>(&payload) {
                Ok(event) => Some(Ok(event)),
                Err(error) => {
                    tracing::warn!(%error, payload, "skipping a malformed switch-profile event");
                    None
                }
            },
            Err(error) => Some(Err(error)),
        }
    }))
}
