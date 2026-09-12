//! The central registry: one proxy slot per subsystem.

use std::fmt;
use std::sync::Arc;

use crate::push::Push;
use crate::slot::ProxySlot;
use crate::traits::{
    BackgroundTasks, CatalogSink, MenuSink, RouteRegistrar, ShutdownHook, StateProvider,
    StatusChannel, StatusSink, WorkspaceRoots,
};

/// The central registry subsystems self-register into.
///
/// Clones are cheap (every slot is an `Arc`) and share the same slots,
/// so every handle the composition root hands out sees the same
/// registrations.
pub struct Registry {
    routes: ProxySlot<dyn RouteRegistrar>,
    state_handles: ProxySlot<dyn StateProvider>,
    tasks: ProxySlot<dyn BackgroundTasks>,
    status: ProxySlot<dyn StatusChannel>,
    shutdown: ProxySlot<dyn ShutdownHook>,
    status_sink: ProxySlot<dyn StatusSink>,
    catalog_sink: ProxySlot<dyn CatalogSink>,
    menu_sink: ProxySlot<dyn MenuSink>,
    session_routes: ProxySlot<dyn RouteRegistrar>,
    workspace_routes: ProxySlot<dyn RouteRegistrar>,
    status_state: ProxySlot<dyn StateProvider>,
    menu_state: ProxySlot<dyn StateProvider>,
    gateway_state: ProxySlot<dyn StateProvider>,
    sessions_state: ProxySlot<dyn StateProvider>,
    workspace_roots: ProxySlot<dyn WorkspaceRoots>,
}

impl Registry {
    /// An empty registry: every slot a graceful no-op until its
    /// subsystem registers.
    #[must_use]
    pub fn new() -> Self {
        Self {
            routes: ProxySlot::new(),
            state_handles: ProxySlot::new(),
            tasks: ProxySlot::new(),
            status: ProxySlot::new(),
            shutdown: ProxySlot::new(),
            status_sink: ProxySlot::new(),
            catalog_sink: ProxySlot::new(),
            menu_sink: ProxySlot::new(),
            session_routes: ProxySlot::new(),
            workspace_routes: ProxySlot::new(),
            status_state: ProxySlot::new(),
            menu_state: ProxySlot::new(),
            gateway_state: ProxySlot::new(),
            sessions_state: ProxySlot::new(),
            workspace_roots: ProxySlot::new(),
        }
    }

    /// The route-registration slot: the subsystem merges its routes into
    /// the shell's API router.
    #[must_use]
    pub fn routes(&self) -> &ProxySlot<dyn RouteRegistrar> {
        &self.routes
    }

    /// The state-handle slot: the subsystem publishes the shared handles
    /// its routes and tasks need.
    #[must_use]
    pub fn state_handles(&self) -> &ProxySlot<dyn StateProvider> {
        &self.state_handles
    }

    /// The background-task slot: the subsystem spawns its long-lived
    /// tasks.
    #[must_use]
    pub fn tasks(&self) -> &ProxySlot<dyn BackgroundTasks> {
        &self.tasks
    }

    /// The status push-channel slot: the status subsystem's retained
    /// snapshot plus live subscription.
    #[must_use]
    pub fn status(&self) -> &ProxySlot<dyn StatusChannel> {
        &self.status
    }

    /// The shutdown slot: the subsystem's graceful-stop signal.
    #[must_use]
    pub fn shutdown(&self) -> &ProxySlot<dyn ShutdownHook> {
        &self.shutdown
    }

    /// The registered status channel, or `None` while the status
    /// subsystem has not registered - a graceful no-op for the `/ws`
    /// session loop.
    #[must_use]
    pub fn status_channel(&self) -> Option<Arc<dyn StatusChannel>> {
        self.status.get()
    }

    /// The status producer slot: the status subsystem registers its
    /// receiving end, and same-tier producers emit through it.
    #[must_use]
    pub fn status_sink(&self) -> &ProxySlot<dyn StatusSink> {
        &self.status_sink
    }

    /// The catalog producer slot: the menu subsystem registers its
    /// catalog channel's receiving end.
    #[must_use]
    pub fn catalog_sink(&self) -> &ProxySlot<dyn CatalogSink> {
        &self.catalog_sink
    }

    /// The menu producer slot: the menu subsystem registers its
    /// workbench mutators' receiving end.
    #[must_use]
    pub fn menu_sink(&self) -> &ProxySlot<dyn MenuSink> {
        &self.menu_sink
    }

    /// The sessions subsystem's route slot: its `/ws`, `/agents/ws`, and
    /// catalog-relay routes, merged into the shell's API router.
    #[must_use]
    pub fn session_routes(&self) -> &ProxySlot<dyn RouteRegistrar> {
        &self.session_routes
    }

    /// The workspace subsystem's route slot: its `/workspace/*` routes,
    /// merged into the shell's API router.
    #[must_use]
    pub fn workspace_routes(&self) -> &ProxySlot<dyn RouteRegistrar> {
        &self.workspace_routes
    }

    /// The status subsystem's state-handle slot: its bus, fetched by the
    /// composition root's consumers through a downcast.
    #[must_use]
    pub fn status_state(&self) -> &ProxySlot<dyn StateProvider> {
        &self.status_state
    }

    /// The menu subsystem's state-handle slot: its catalog and menu
    /// buses, fetched by the composition root's consumers through a
    /// downcast.
    #[must_use]
    pub fn menu_state(&self) -> &ProxySlot<dyn StateProvider> {
        &self.menu_state
    }

    /// The gateway subsystem's state-handle slot: its endpoint binding
    /// and health flag, fetched through a downcast.
    #[must_use]
    pub fn gateway_state(&self) -> &ProxySlot<dyn StateProvider> {
        &self.gateway_state
    }

    /// The sessions subsystem's state-handle slot: the agent-session
    /// registry, fetched through a downcast.
    #[must_use]
    pub fn sessions_state(&self) -> &ProxySlot<dyn StateProvider> {
        &self.sessions_state
    }

    /// The workspace subsystem's granted-roots slot: the narrow state
    /// handle same-tier subsystems read instead of naming the workspace
    /// crate.
    #[must_use]
    pub fn workspace_roots(&self) -> &ProxySlot<dyn WorkspaceRoots> {
        &self.workspace_roots
    }

    /// The intent-named push facade over the producer sink slots, for
    /// subsystems that report what happened without naming another
    /// subsystem's bus.
    #[must_use]
    pub fn push(&self) -> Push {
        Push::new(self.clone())
    }
}

impl Default for Registry {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for Registry {
    fn clone(&self) -> Self {
        Self {
            routes: self.routes.clone(),
            state_handles: self.state_handles.clone(),
            tasks: self.tasks.clone(),
            status: self.status.clone(),
            shutdown: self.shutdown.clone(),
            status_sink: self.status_sink.clone(),
            catalog_sink: self.catalog_sink.clone(),
            menu_sink: self.menu_sink.clone(),
            session_routes: self.session_routes.clone(),
            workspace_routes: self.workspace_routes.clone(),
            status_state: self.status_state.clone(),
            menu_state: self.menu_state.clone(),
            gateway_state: self.gateway_state.clone(),
            sessions_state: self.sessions_state.clone(),
            workspace_roots: self.workspace_roots.clone(),
        }
    }
}

impl fmt::Debug for Registry {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Registry")
            .field("routes", &self.routes)
            .field("state_handles", &self.state_handles)
            .field("tasks", &self.tasks)
            .field("status", &self.status)
            .field("shutdown", &self.shutdown)
            .field("status_sink", &self.status_sink)
            .field("catalog_sink", &self.catalog_sink)
            .field("menu_sink", &self.menu_sink)
            .field("session_routes", &self.session_routes)
            .field("workspace_routes", &self.workspace_routes)
            .field("status_state", &self.status_state)
            .field("menu_state", &self.menu_state)
            .field("gateway_state", &self.gateway_state)
            .field("sessions_state", &self.sessions_state)
            .field("workspace_roots", &self.workspace_roots)
            .finish()
    }
}
