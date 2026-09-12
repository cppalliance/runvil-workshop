//! Shared handler state and the composition root that assembles the
//! per-feature routers into the workshop server.
//!
//! [`AppState`] holds no subsystem state by name: each extracted
//! subsystem owns its state behind a narrow handle registered into the
//! [`Registry`], and consumers fetch the handles through the registry's
//! slots. What remains here is the shell's own runtime infrastructure -
//! the shared reconnect backoff and the process progress hub - plus the
//! registration guards keeping every self-registration alive.

#[cfg(any(test, feature = "test-fixtures"))]
pub(crate) mod fixtures;
#[cfg(test)]
mod tests;

use std::any::Any;
use std::fmt;
use std::sync::Arc;

use axum::Router;

use shared_progress::ProgressHub;

use workshop_gateway::GatewayHandles;
use workshop_menu::MenuHandles;
use workshop_registry::{ProxySlot, Push, Registration, Registry, StateProvider};
use workshop_sessions::{AgentSessions, SessionHost, SessionsState};
use workshop_status::StatusBus;
use workshop_support::{Config, DEFAULT_DEADLINE, ReconnectBackoff, with_deadline};

use crate::catalog::CatalogBus;
use crate::gateway::GatewayError;
use crate::gateway_binding::{GatewayBinding, GatewaySnapshot, GatewayUpdater};
use crate::heartbeat::GatewayHealth;
use crate::menu::MenuBus;
use crate::resolve::ResolvedGateway;
use crate::routes;

/// Address the server binds to when no override is given.
pub use workshop_support::DEFAULT_ADDR;

/// Shared handler state: the subsystem registry, the shell's runtime
/// infrastructure, and the registration guards. Subsystem handles - the
/// gateway binding and health flag, the status, catalog, and menu buses,
/// the agent-session registry - are fetched through the registry's
/// slots, where each subsystem self-registers them.
#[derive(Debug, Clone)]
pub struct AppState {
    backoff: ReconnectBackoff,
    progress: Arc<ProgressHub>,
    registry: Registry,
    // Keeps the subsystems' self-registrations alive; dropping the last
    // state clone deregisters them.
    _registrations: Registrations,
}

/// The registration guards keeping every subsystem's self-registrations
/// alive: the push channels, the producer sinks, the state handles, the
/// route registrars, and the workspace roots handle.
#[derive(Clone)]
struct Registrations {
    guards: Vec<Arc<dyn Any + Send + Sync>>,
}

impl Registrations {
    fn new() -> Self {
        Self { guards: Vec::new() }
    }

    /// Holds one registration guard for the state's lifetime.
    fn hold<T: ?Sized + Send + Sync + 'static>(&mut self, guard: Registration<T>) {
        self.guards.push(Arc::new(guard));
    }
}

impl fmt::Debug for Registrations {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Registrations")
            .field("held", &self.guards.len())
            .finish()
    }
}

/// Fetches one subsystem's registered handle set from `slot`, downcast
/// to its concrete type. A missing or mistyped registration is a
/// composition-root bug (zone one), never a runtime condition: the root
/// registers every subsystem before sharing state.
fn registered<T>(slot: &ProxySlot<dyn StateProvider>, what: &str) -> T
where
    T: Clone + Send + Sync + 'static,
{
    slot.get()
        .and_then(|provider| provider.handles().downcast::<T>().ok())
        .map_or_else(
            || panic!("the composition root registers {what} before sharing state"),
            |handles| (*handles).clone(),
        )
}

impl AppState {
    /// Builds shared state from the loaded configuration, resolving the
    /// gateway endpoint first: a live gateway discovery file in the run
    /// directory wins over explicit `[gateway]` config.
    ///
    /// # Errors
    /// Returns [`StateError::Resolution`] when no live gateway discovery file
    /// exists and the config carries no explicit gateway, and
    /// [`StateError::Gateway`] if the HTTP client cannot be built.
    pub fn new(config: &Config) -> Result<Self, StateError> {
        let gateway = crate::resolve::resolve(&config.gateway).map_err(StateError::Resolution)?;
        state_with_gateway(config, &gateway)
    }

    /// The status bus, which every `/ws` session subscribes to so it can
    /// forward updates; producers report through [`AppState::push`].
    #[must_use]
    pub fn status(&self) -> StatusBus {
        registered(self.registry.status_state(), "the status bus")
    }

    /// The process progress hub: operations with bounded lifetimes attach
    /// trees to it as they run, and the renderer task (spawned with the
    /// server) turns its snapshots into the status bar's progress
    /// indicator.
    pub(crate) fn progress(&self) -> &Arc<ProgressHub> {
        &self.progress
    }

    /// The push facade over the status, catalog, and menu sink slots,
    /// held by every subsystem that reports what happened.
    #[must_use]
    pub fn push(&self) -> Push {
        self.registry.push()
    }

    /// The gateway subsystem's registered handles.
    fn gateway_handles(&self) -> GatewayHandles {
        registered(self.registry.gateway_state(), "the gateway handles")
    }

    /// The menu subsystem's registered handles.
    fn menu_handles(&self) -> MenuHandles {
        registered(self.registry.menu_state(), "the menu handles")
    }

    /// One atomic Gateway endpoint and credential generation.
    pub(crate) fn gateway_snapshot(&self) -> Arc<GatewaySnapshot> {
        self.gateway_handles().binding().snapshot()
    }

    /// A clone of the currently published Gateway HTTP client.
    #[must_use]
    pub fn gateway_client(&self) -> crate::GatewayClient {
        self.gateway_snapshot().client().clone()
    }

    /// The replaceable Gateway binding shared with long-lived tasks.
    pub(crate) fn gateway_binding(&self) -> GatewayBinding {
        self.gateway_handles().binding().clone()
    }

    /// Restricted local-Gateway authority for an embedding host.
    pub(crate) fn gateway_updater(&self) -> GatewayUpdater {
        self.gateway_handles().binding().updater()
    }

    /// Permanently revokes host publication before application teardown.
    pub(crate) fn close_gateway_publication(&self) {
        self.gateway_handles()
            .binding()
            .updater()
            .close_publication();
    }

    /// Shared gateway reachability, published by the heartbeat; the
    /// gateway-dependent routes read it to short-circuit while the gateway
    /// is down.
    #[must_use]
    pub fn health(&self) -> GatewayHealth {
        self.gateway_handles().health().clone()
    }

    /// The shared reconnect backoff: the heartbeat draws probe delays
    /// from it while the gateway is down, and the agent sessions reset it
    /// on useful work - a completed model reply.
    #[must_use]
    pub fn backoff(&self) -> ReconnectBackoff {
        self.backoff.clone()
    }

    /// The catalog bus, which the heartbeat publishes the refreshed model
    /// catalog to on a gateway reconnect and every `/ws` session forwards
    /// from.
    #[must_use]
    pub fn catalog(&self) -> CatalogBus {
        self.menu_handles().catalog().clone()
    }

    /// The menu bus, whose workbench snapshots every `/ws` session
    /// forwards and whose mutators the session's menu events drive.
    #[must_use]
    pub fn menu(&self) -> MenuBus {
        self.menu_handles().menu().clone()
    }

    /// The subsystem registry: proxy slots the subsystems self-register
    /// into, so consumers reach them by slot instead of by name.
    #[must_use]
    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    /// The agent-session registry: discovery, launch, and the running
    /// sessions behind the `/agents/ws` socket. Sessions outlive
    /// sockets, so an embedding host ends one through
    /// [`AgentSessions::close`].
    #[must_use]
    pub fn agents(&self) -> AgentSessions {
        registered(self.registry.sessions_state(), "the agent-session registry")
    }
}

/// Builds shared state against an already-resolved gateway endpoint: the
/// construction phase a host holding its own endpoint enters directly,
/// skipping gateway discovery file resolution.
///
/// The composition root: every subsystem is constructed here, registers
/// itself into the registry, and is thereafter reached through the
/// registry's slots - routes included, which [`router`] merges from the
/// route slots.
///
/// # Errors
/// Returns [`StateError::Gateway`] if the HTTP client cannot be built.
pub fn state_with_gateway(
    config: &Config,
    gateway: &ResolvedGateway,
) -> Result<AppState, StateError> {
    let status = StatusBus::new();
    let catalog = CatalogBus::new();
    // The per-profile model memory lives in the state directory; a bad
    // or missing memory file costs the memory, never startup.
    let state_dir = &config.server.state_dir;
    // A crash between an atomic write's temp file and its rename
    // orphans the temp; boot is the one moment the directory is
    // known and quiet, so it is swept here.
    workshop_support::sweep_orphaned_temps(state_dir);
    let menu = MenuBus::new(catalog.clone(), Some(state_dir));
    // The subsystems self-register: consumers reach their channels,
    // sinks, state handles, and routes through the registry's slots
    // instead of by name.
    let registry = Registry::new();
    let mut registrations = Registrations::new();
    let (status_channel, status_sink, status_state) = workshop_status::register(&registry, &status);
    registrations.hold(status_channel);
    registrations.hold(status_sink);
    registrations.hold(status_state);
    let (catalog_sink, menu_sink, menu_state) = workshop_menu::register(&registry, &catalog, &menu);
    registrations.hold(catalog_sink);
    registrations.hold(menu_sink);
    registrations.hold(menu_state);
    let push = registry.push();
    // Startup phases are reported as they run; with no client connected
    // yet these land on an empty bus, ready for the first session.
    crate::resolve::report(gateway, &push);
    let gateway_binding = GatewayBinding::new_with_identity(
        gateway.base_url(),
        gateway.api_key(),
        gateway.identity().cloned(),
    )
    .map_err(StateError::Gateway)?;
    let progress = Arc::new(ProgressHub::new());
    let backoff = ReconnectBackoff::new();
    let health = GatewayHealth::new();
    registrations.hold(workshop_gateway::register(
        &registry,
        GatewayHandles::new(gateway_binding.clone(), health.clone()),
    ));
    let workspace = workshop_workspace::Workspace::new();
    let (workspace_routes, workspace_roots) = workshop_workspace::register(&registry, &workspace);
    registrations.hold(workspace_routes);
    registrations.hold(workspace_roots);
    let agents = AgentSessions::new(
        config.agents.path.clone(),
        state_dir.join("sessions"),
        gateway_binding.clone(),
        SessionHost::new(
            registry.clone(),
            backoff.clone(),
            menu.clone(),
            catalog.clone(),
        ),
    );
    let sessions = SessionsState::new(
        agents,
        gateway_binding,
        health,
        catalog,
        menu,
        registry.clone(),
        crate::cross_site::origin_allowed,
    );
    let (session_routes, sessions_state) = workshop_sessions::register(&registry, sessions);
    registrations.hold(session_routes);
    registrations.hold(sessions_state);
    push.push_idle();
    Ok(AppState {
        backoff,
        progress,
        registry,
        _registrations: registrations,
    })
}

/// A shared-state construction failure: rich, init-only, and never sent
/// over the wire (the HTTP failure type is `crate::error::AppError`).
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum StateError {
    /// The gateway HTTP client could not be built.
    #[non_exhaustive]
    #[error("build gateway client")]
    Gateway(#[source] GatewayError),

    /// No gateway endpoint could be resolved: no live gateway discovery file and
    /// no explicit `[gateway]` config.
    #[non_exhaustive]
    #[error("resolve the gateway endpoint")]
    Resolution(#[source] crate::resolve::ResolveError),
}

/// Returns the workshop server router with every route mounted: the
/// shell's own feature routers from [`crate::routes`], plus the extracted
/// subsystems' routers merged from the registry's route slots - an
/// unregistered slot is a graceful no-op. The API routes sit behind the
/// `crate::cross_site` guard; `/health` and the UI assets stay outside it
/// so the shell probe, heartbeat, and initial navigation keep working.
/// Every response carries the `crate::csp` policy: the shell's webview
/// loads the UI as an External origin, so the server sets the page's
/// Content-Security-Policy. Each subsystem applies its own deadline tier:
/// the default on the workspace routes, the relay tier on `/v1/models`,
/// none on the WebSocket upgrades.
pub fn router(state: AppState) -> Router {
    let registry = state.registry().clone();
    let mut api = Router::new()
        .merge(routes::realtime::routes(state.clone()))
        .merge(routes::gateway_config::routes(state));
    if let Some(registrar) = registry.session_routes().get() {
        api = api.merge(registrar.routes());
    }
    if let Some(registrar) = registry.workspace_routes().get() {
        api = api.merge(registrar.routes());
    }
    let api = api.layer(axum::middleware::from_fn(crate::cross_site::guard));
    Router::new()
        .merge(with_deadline(routes::assets::routes(), DEFAULT_DEADLINE))
        .merge(with_deadline(routes::health::routes(), DEFAULT_DEADLINE))
        .merge(api)
        // The outermost layer on the server's own routes: every response
        // carries the CSP, error envelopes included, so the shell's
        // External-origin webview runs under the policy no matter which
        // route answered.
        .layer(axum::middleware::from_fn(crate::csp::header))
}
