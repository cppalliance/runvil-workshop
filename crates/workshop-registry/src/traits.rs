//! The sealed subsystem traits: one per registration point in the
//! decomposition's inventory (routes, state handles, background tasks,
//! push channels, shutdown).
//!
//! All five are sealed behind a private supertrait, so only this crate
//! can implement them. A registrant plugs its subsystem in through an
//! adapter this crate provides - [`StatusChannelAdapter`] is the first,
//! added with the status bus's proof-of-concept migration; each later
//! migration adds its adapter beside it.

use std::any::Any;
use std::fmt;
use std::sync::Arc;

use axum::Router;
use tokio::sync::broadcast;
use tokio::task::JoinHandle;

use workshop_protocol::StatusBarUpdate;

/// The sealing boundary: a private empty supertrait no downstream crate
/// can name, so the subsystem traits below cannot be implemented outside
/// this crate.
mod sealed {
    /// The private empty supertrait sealing every subsystem trait.
    pub trait Sealed {}
}
use sealed::Sealed;

/// Route registration: a subsystem contributes its HTTP routes, merged
/// into the shell's API router at composition time.
pub trait RouteRegistrar: Sealed + Send + Sync {
    /// The subsystem's routes, with their state already applied.
    fn routes(&self) -> Router;
}

/// State handle provision: a subsystem publishes the shared handles its
/// routes and tasks need, type-erased so the shell's state object
/// shrinks to a registry of handles.
pub trait StateProvider: Sealed + Send + Sync {
    /// The subsystem's handle set, downcast by its consumers.
    fn handles(&self) -> Arc<dyn Any + Send + Sync>;
}

/// Background task spawning: a subsystem starts its long-lived tasks,
/// so the composition root holds no `tokio::spawn` calls of its own.
pub trait BackgroundTasks: Sealed + Send + Sync {
    /// Spawns the subsystem's tasks on `runtime`; the returned join
    /// handles are the shell's shutdown lever.
    fn spawn(&self, runtime: &tokio::runtime::Handle) -> Vec<JoinHandle<()>>;
}

/// The status-bar push channel: the retained snapshot plus live
/// subscription every `/ws` session forwards.
pub trait StatusChannel: Sealed + Send + Sync {
    /// Subscribes to every update sent from this call onward.
    fn subscribe(&self) -> broadcast::Receiver<StatusBarUpdate>;
    /// The most recently emitted update, retained so a session
    /// connecting later can send the current status as its snapshot.
    fn latest(&self) -> Option<StatusBarUpdate>;
}

/// Shutdown handle: a subsystem's graceful-stop signal, held by the
/// shell and fired at teardown.
pub trait ShutdownHook: Sealed + Send + Sync {
    /// Signals the subsystem to stop; returns immediately.
    fn shutdown(&self);
}

/// A [`StatusChannel`] backed by two closures over the status bus: the
/// registration adapter for the status subsystem. The registry's traits
/// are sealed, so the registrant plugs its bus in through this adapter
/// rather than implementing the trait itself.
pub struct StatusChannelAdapter<S, L> {
    subscribe: S,
    latest: L,
}

impl<S, L> StatusChannelAdapter<S, L>
where
    S: Fn() -> broadcast::Receiver<StatusBarUpdate> + Send + Sync,
    L: Fn() -> Option<StatusBarUpdate> + Send + Sync,
{
    /// Builds the adapter from the bus's subscribe and latest closures.
    pub fn new(subscribe: S, latest: L) -> Self {
        Self { subscribe, latest }
    }
}

impl<S, L> Sealed for StatusChannelAdapter<S, L>
where
    S: Fn() -> broadcast::Receiver<StatusBarUpdate> + Send + Sync,
    L: Fn() -> Option<StatusBarUpdate> + Send + Sync,
{
}

impl<S, L> StatusChannel for StatusChannelAdapter<S, L>
where
    S: Fn() -> broadcast::Receiver<StatusBarUpdate> + Send + Sync,
    L: Fn() -> Option<StatusBarUpdate> + Send + Sync,
{
    fn subscribe(&self) -> broadcast::Receiver<StatusBarUpdate> {
        (self.subscribe)()
    }

    fn latest(&self) -> Option<StatusBarUpdate> {
        (self.latest)()
    }
}

impl<S, L> fmt::Debug for StatusChannelAdapter<S, L> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("StatusChannelAdapter").finish()
    }
}
