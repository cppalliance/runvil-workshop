//! Catalog frames: the `{"type":"models",...}` pushes.

use serde::Serialize;

/// One pushed model catalog.
#[derive(Debug, Clone, PartialEq)]
pub struct CatalogPush {
    /// The chat-capable subset of the gateway's model array.
    pub models: Vec<serde_json::Value>,
}

impl CatalogPush {
    /// The push as a wire frame: `"type": "models"` beside the array.
    #[must_use]
    pub fn frame(&self) -> CatalogFrame<'_> {
        CatalogFrame {
            kind: "models",
            models: &self.models,
        }
    }
}

/// The serialized shape of a catalog push on the socket, matching the
/// workshop protocol's frame taxonomy.
///
/// Delivery: ephemeral - the newest push carries the whole catalog and
/// supersedes every older one; the catalog is resent on reconnect.
#[derive(Debug, Serialize)]
pub struct CatalogFrame<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    models: &'a [serde_json::Value],
}
