//! The sessions subsystem's shared route state and its registry
//! registration: the state every sessions route handler draws its
//! handles from, the router constructor, and the `register` entry point
//! the composition root calls.

use std::sync::Arc;

use axum::Router;
use axum::http::HeaderMap;
use axum::routing::get;

use workshop_gateway::{GatewayBinding, GatewayHealth, GatewaySnapshot};
use workshop_menu::{CatalogBus, MenuBus};
use workshop_registry::{
    Push, Registration, Registry, RouteRegistrar, RouteRegistrarAdapter, StateProvider,
    StateProviderAdapter,
};
use workshop_support::{RELAY_DEADLINE, with_deadline};

use crate::agents::AgentSessions;
use crate::{agents, relay, session};

/// The shared state of the sessions subsystem's routes: the agent-session
/// registry, the gateway endpoint binding and reachability flag, the
/// catalog and menu buses, the subsystem registry (the status channel and
/// the push facade), and the shell's WebSocket origin policy.
///
/// The origin policy is injected by the shell as a plain function: the
/// cross-site guard is the shell's security boundary (its `cross_site`
/// module), and the subsystem applies it to every upgrade without owning
/// the policy.
#[derive(Debug, Clone)]
pub struct SessionsState {
    agents: AgentSessions,
    gateway: GatewayBinding,
    health: GatewayHealth,
    catalog: CatalogBus,
    menu: MenuBus,
    registry: Registry,
    origin_allowed: fn(&HeaderMap) -> bool,
}

impl SessionsState {
    /// Builds the route state from the subsystem's handles.
    #[must_use]
    pub fn new(
        agents: AgentSessions,
        gateway: GatewayBinding,
        health: GatewayHealth,
        catalog: CatalogBus,
        menu: MenuBus,
        registry: Registry,
        origin_allowed: fn(&HeaderMap) -> bool,
    ) -> Self {
        Self {
            agents,
            gateway,
            health,
            catalog,
            menu,
            registry,
            origin_allowed,
        }
    }

    /// The agent-session registry behind `/agents/ws`.
    pub(crate) fn agents(&self) -> &AgentSessions {
        &self.agents
    }

    /// One atomic Gateway endpoint and credential generation.
    pub(crate) fn gateway_snapshot(&self) -> Arc<GatewaySnapshot> {
        self.gateway.snapshot()
    }

    /// Shared gateway reachability, published by the heartbeat.
    pub(crate) fn health(&self) -> &GatewayHealth {
        &self.health
    }

    /// The catalog bus every `/ws` session forwards from.
    pub(crate) fn catalog(&self) -> &CatalogBus {
        &self.catalog
    }

    /// The menu bus every `/ws` session forwards and drives.
    pub(crate) fn menu(&self) -> &MenuBus {
        &self.menu
    }

    /// The subsystem registry: the status push channel and the push
    /// facade are reached through its slots.
    pub(crate) fn registry(&self) -> &Registry {
        &self.registry
    }

    /// The push facade over the status, catalog, and menu sink slots.
    pub(crate) fn push(&self) -> Push {
        self.registry.push()
    }

    /// The shell's WebSocket origin policy, applied to every upgrade.
    pub(crate) fn origin_allowed(&self, headers: &HeaderMap) -> bool {
        (self.origin_allowed)(headers)
    }
}

/// The sessions subsystem's routes: the `/v1/models` catalog relay on the
/// relay deadline, and the `/ws` and `/agents/ws` WebSocket upgrades,
/// which answer immediately and then outlive any deadline.
pub fn routes(state: SessionsState) -> Router {
    with_deadline(
        Router::new().route("/v1/models", get(relay::models)),
        RELAY_DEADLINE,
    )
    .route("/ws", get(session::upgrade))
    .route("/agents/ws", get(agents::socket::upgrade))
    .with_state(state)
}

/// Registers the sessions subsystem into the registry: its routes, merged
/// into the shell's API router, and the agent-session registry as its
/// state handle. The returned guards keep the registrations alive; the
/// composition root holds them for the process lifetime.
pub fn register(
    registry: &Registry,
    state: SessionsState,
) -> (
    Registration<dyn RouteRegistrar>,
    Registration<dyn StateProvider>,
) {
    let routes = registry
        .session_routes()
        .register(Arc::new(RouteRegistrarAdapter::new({
            let state = state.clone();
            move || routes(state.clone())
        })));
    let handles = registry
        .sessions_state()
        .register(Arc::new(StateProviderAdapter::new(move || {
            Arc::new(state.agents().clone()) as Arc<dyn std::any::Any + Send + Sync>
        })));
    (routes, handles)
}
