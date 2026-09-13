//! The workspace subsystem's registration: its `/workspace/*` routes,
//! merged into the shell's API router, and its granted-roots state
//! handle, which same-tier subsystems read instead of naming this
//! crate.

use std::sync::Arc;

use workshop_registry::{
    Registration, Registry, RouteRegistrar, RouteRegistrarAdapter, WorkspaceRoots,
    WorkspaceRootsAdapter,
};

use crate::handlers;
use crate::workspace::Workspace;

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
