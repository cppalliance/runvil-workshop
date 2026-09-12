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
use std::path::PathBuf;
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

/// The status producer sink: the status subsystem's receiving end for
/// [`StatusBarUpdate`]s emitted by subsystems in other crates. The
/// [`Push`](crate::Push) facade builds the frames; the sink only
/// accepts them, so a producer in a same-tier crate never names the
/// status bus's type.
pub trait StatusSink: Sealed + Send + Sync {
    /// Emits one update onto the status bus.
    fn emit(&self, update: StatusBarUpdate);
}

/// The catalog producer sink: the menu subsystem's receiving end for
/// refreshed model catalogs published by the gateway subsystem.
pub trait CatalogSink: Sealed + Send + Sync {
    /// Publishes one complete model catalog snapshot.
    fn publish(&self, models: Vec<serde_json::Value>);
}

/// The menu producer sink: the menu subsystem's receiving end for the
/// workbench mutators the gateway subsystem drives - reachability
/// verdicts, profile state, and selection restores.
pub trait MenuSink: Sealed + Send + Sync {
    /// Records the heartbeat's verdict on gateway reachability.
    fn set_gateway_reachable(&self, reachable: bool);
    /// Records the gateway's profile list and active profile.
    fn set_profiles(&self, profiles: Vec<String>, active: Option<String>);
    /// Restores a boot-time selection when none is applied.
    fn restore_selection(&self);
    /// Revalidates the selection against the catalog just published.
    fn reconcile_catalog(&self);
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

/// A [`StatusSink`] backed by one closure over the status bus: the
/// registration adapter for the status subsystem's producer side. The
/// registry's traits are sealed, so the registrant plugs its bus in
/// through this adapter rather than implementing the trait itself.
pub struct StatusSinkAdapter<E> {
    emit: E,
}

impl<E> StatusSinkAdapter<E>
where
    E: Fn(StatusBarUpdate) + Send + Sync,
{
    /// Builds the adapter from the bus's emit closure.
    pub fn new(emit: E) -> Self {
        Self { emit }
    }
}

impl<E> Sealed for StatusSinkAdapter<E> where E: Fn(StatusBarUpdate) + Send + Sync {}

impl<E> StatusSink for StatusSinkAdapter<E>
where
    E: Fn(StatusBarUpdate) + Send + Sync,
{
    fn emit(&self, update: StatusBarUpdate) {
        (self.emit)(update);
    }
}

impl<E> fmt::Debug for StatusSinkAdapter<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("StatusSinkAdapter").finish()
    }
}

/// A [`CatalogSink`] backed by one closure over the catalog bus: the
/// registration adapter for the menu subsystem's catalog channel.
pub struct CatalogSinkAdapter<P> {
    publish: P,
}

impl<P> CatalogSinkAdapter<P>
where
    P: Fn(Vec<serde_json::Value>) + Send + Sync,
{
    /// Builds the adapter from the bus's publish closure.
    pub fn new(publish: P) -> Self {
        Self { publish }
    }
}

impl<P> Sealed for CatalogSinkAdapter<P> where P: Fn(Vec<serde_json::Value>) + Send + Sync {}

impl<P> CatalogSink for CatalogSinkAdapter<P>
where
    P: Fn(Vec<serde_json::Value>) + Send + Sync,
{
    fn publish(&self, models: Vec<serde_json::Value>) {
        (self.publish)(models);
    }
}

impl<P> fmt::Debug for CatalogSinkAdapter<P> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("CatalogSinkAdapter").finish()
    }
}

/// The workspace subsystem's granted-root view: the narrow state handle
/// same-tier subsystems (the agent session's `ui()` snapshot) consume
/// through the registry instead of naming the workspace crate, which the
/// one-way tier graph forbids.
pub trait WorkspaceRoots: Sealed + Send + Sync {
    /// The granted workspace roots in stable sorted order.
    fn granted_roots(&self) -> Vec<PathBuf>;
}

/// A [`WorkspaceRoots`] backed by one closure over the workspace's grant
/// set: the registration adapter for the workspace subsystem. The
/// registry's traits are sealed, so the registrant plugs its state in
/// through this adapter rather than implementing the trait itself.
pub struct WorkspaceRootsAdapter<F> {
    roots: F,
}

impl<F> WorkspaceRootsAdapter<F>
where
    F: Fn() -> Vec<PathBuf> + Send + Sync,
{
    /// Builds the adapter from the workspace's granted-roots closure.
    pub fn new(roots: F) -> Self {
        Self { roots }
    }
}

impl<F> Sealed for WorkspaceRootsAdapter<F> where F: Fn() -> Vec<PathBuf> + Send + Sync {}

impl<F> WorkspaceRoots for WorkspaceRootsAdapter<F>
where
    F: Fn() -> Vec<PathBuf> + Send + Sync,
{
    fn granted_roots(&self) -> Vec<PathBuf> {
        (self.roots)()
    }
}

impl<F> fmt::Debug for WorkspaceRootsAdapter<F> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("WorkspaceRootsAdapter").finish()
    }
}

/// A [`RouteRegistrar`] backed by one closure building the subsystem's
/// router: the registration adapter for a subsystem's routes. The
/// registry's traits are sealed, so the registrant plugs its routes in
/// through this adapter rather than implementing the trait itself.
pub struct RouteRegistrarAdapter<F> {
    build: F,
}

impl<F> RouteRegistrarAdapter<F>
where
    F: Fn() -> Router + Send + Sync,
{
    /// Builds the adapter from the subsystem's router constructor.
    pub fn new(build: F) -> Self {
        Self { build }
    }
}

impl<F> Sealed for RouteRegistrarAdapter<F> where F: Fn() -> Router + Send + Sync {}

impl<F> RouteRegistrar for RouteRegistrarAdapter<F>
where
    F: Fn() -> Router + Send + Sync,
{
    fn routes(&self) -> Router {
        (self.build)()
    }
}

impl<F> fmt::Debug for RouteRegistrarAdapter<F> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("RouteRegistrarAdapter").finish()
    }
}

/// A [`StateProvider`] backed by one closure yielding the subsystem's
/// handle set: the registration adapter for a subsystem's shared state.
/// The registry's traits are sealed, so the registrant plugs its handles
/// in through this adapter rather than implementing the trait itself.
pub struct StateProviderAdapter<F> {
    handles: F,
}

impl<F> StateProviderAdapter<F>
where
    F: Fn() -> Arc<dyn Any + Send + Sync> + Send + Sync,
{
    /// Builds the adapter from the subsystem's handle-set closure.
    pub fn new(handles: F) -> Self {
        Self { handles }
    }
}

impl<F> Sealed for StateProviderAdapter<F> where F: Fn() -> Arc<dyn Any + Send + Sync> + Send + Sync {}

impl<F> StateProvider for StateProviderAdapter<F>
where
    F: Fn() -> Arc<dyn Any + Send + Sync> + Send + Sync,
{
    fn handles(&self) -> Arc<dyn Any + Send + Sync> {
        (self.handles)()
    }
}

impl<F> fmt::Debug for StateProviderAdapter<F> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("StateProviderAdapter").finish()
    }
}

/// A [`MenuSink`] backed by closures over the menu bus's mutators: the
/// registration adapter for the menu subsystem's workbench state.
pub struct MenuSinkAdapter<R, P, S, C> {
    reachable: R,
    profiles: P,
    restore: S,
    reconcile: C,
}

impl<R, P, S, C> MenuSinkAdapter<R, P, S, C>
where
    R: Fn(bool) + Send + Sync,
    P: Fn(Vec<String>, Option<String>) + Send + Sync,
    S: Fn() + Send + Sync,
    C: Fn() + Send + Sync,
{
    /// Builds the adapter from the menu bus's mutator closures, in the
    /// [`MenuSink`] trait's method order.
    pub fn new(reachable: R, profiles: P, restore: S, reconcile: C) -> Self {
        Self {
            reachable,
            profiles,
            restore,
            reconcile,
        }
    }
}

impl<R, P, S, C> Sealed for MenuSinkAdapter<R, P, S, C>
where
    R: Fn(bool) + Send + Sync,
    P: Fn(Vec<String>, Option<String>) + Send + Sync,
    S: Fn() + Send + Sync,
    C: Fn() + Send + Sync,
{
}

impl<R, P, S, C> MenuSink for MenuSinkAdapter<R, P, S, C>
where
    R: Fn(bool) + Send + Sync,
    P: Fn(Vec<String>, Option<String>) + Send + Sync,
    S: Fn() + Send + Sync,
    C: Fn() + Send + Sync,
{
    fn set_gateway_reachable(&self, reachable: bool) {
        (self.reachable)(reachable);
    }

    fn set_profiles(&self, profiles: Vec<String>, active: Option<String>) {
        (self.profiles)(profiles, active);
    }

    fn restore_selection(&self) {
        (self.restore)();
    }

    fn reconcile_catalog(&self) {
        (self.reconcile)();
    }
}

impl<R, P, S, C> fmt::Debug for MenuSinkAdapter<R, P, S, C> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("MenuSinkAdapter").finish()
    }
}
