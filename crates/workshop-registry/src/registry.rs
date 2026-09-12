//! The central registry: one proxy slot per subsystem.

use std::fmt;
use std::sync::Arc;

use crate::slot::ProxySlot;
use crate::traits::{BackgroundTasks, RouteRegistrar, ShutdownHook, StateProvider, StatusChannel};

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
            .finish()
    }
}
