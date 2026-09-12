//! workshop-workspace - the workspace subsystem: confined filesystem
//! access behind `/workspace/*` - directory trees, file reads, and file
//! writes jailed to roots explicitly granted through drag and drop.
//!
//! ## Invariants
//!
//! - Tier: feature; may depend on: `workshop-protocol`,
//!   `workshop-registry`, `workshop-support`, and the service crates.
//!   Read `AGENTS.md` before adding an import.
//! - Every file in this crate stays under 500 lines; split first, then
//!   edit.
//! - Every request path is checked lexically (no `..`, and on Windows no
//!   NTFS alternate data stream names) and then canonicalized and
//!   prefix-matched against the canonical grants before any filesystem
//!   operation, so traversal, symlink escapes, and UNC aliases cannot
//!   reach outside a grant.
//! - Grants live in memory for the running process only - profile
//!   persistence is a separate future consent decision.
//! - The crate maps its own [`WorkspaceError`] to the wire envelope at
//!   its route boundary; no shell error type appears here.

mod error;
mod handlers;
mod workspace;

use std::sync::Arc;

pub use error::WorkspaceError;
pub use handlers::routes;
use workshop_registry::{
    Registration, Registry, RouteRegistrar, RouteRegistrarAdapter, WorkspaceRoots,
    WorkspaceRootsAdapter,
};
pub use workspace::{EntryKind, FileContents, TreeEntry, TreeListing, Workspace};

/// Registers the workspace subsystem into the registry: its
/// `/workspace/*` routes, merged into the shell's API router, and its
/// granted-roots state handle, which same-tier subsystems read instead
/// of naming this crate. The returned guards keep the registrations
/// alive; the composition root holds them for the process lifetime.
pub fn register(
    registry: &Registry,
    workspace: &Workspace,
) -> (
    Registration<dyn RouteRegistrar>,
    Registration<dyn WorkspaceRoots>,
) {
    let routes = registry
        .workspace_routes()
        .register(Arc::new(RouteRegistrarAdapter::new({
            let workspace = workspace.clone();
            move || handlers::routes(workspace.clone())
        })));
    let roots = registry
        .workspace_roots()
        .register(Arc::new(WorkspaceRootsAdapter::new({
            let workspace = workspace.clone();
            move || workspace.granted_roots()
        })));
    (routes, roots)
}
