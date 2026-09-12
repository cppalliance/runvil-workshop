//! The gateway subsystem's state handles: the replaceable endpoint
//! binding and the reachability flag, bundled for registration into the
//! subsystem registry so the composition root fetches them by slot
//! instead of holding them by name.

use std::sync::Arc;

use workshop_registry::{Registration, Registry, StateProvider, StateProviderAdapter};

use crate::gateway_binding::GatewayBinding;
use crate::heartbeat::GatewayHealth;

/// The gateway subsystem's shared handles: the atomically replaceable
/// endpoint binding every gateway call snapshots, and the heartbeat's
/// reachability flag the gateway-dependent routes short-circuit on.
#[derive(Debug, Clone)]
pub struct GatewayHandles {
    binding: GatewayBinding,
    health: GatewayHealth,
}

impl GatewayHandles {
    /// Bundles the binding and the health flag for registration.
    #[must_use]
    pub fn new(binding: GatewayBinding, health: GatewayHealth) -> Self {
        Self { binding, health }
    }

    /// The replaceable endpoint binding.
    #[must_use]
    pub fn binding(&self) -> &GatewayBinding {
        &self.binding
    }

    /// The heartbeat's shared reachability flag.
    #[must_use]
    pub fn health(&self) -> &GatewayHealth {
        &self.health
    }
}

/// Registers the gateway subsystem's state handles into the registry.
/// The returned guard keeps the registration alive; the composition
/// root holds it for the process lifetime.
pub fn register(registry: &Registry, handles: GatewayHandles) -> Registration<dyn StateProvider> {
    registry
        .gateway_state()
        .register(Arc::new(StateProviderAdapter::new(move || {
            Arc::new(handles.clone()) as Arc<dyn std::any::Any + Send + Sync>
        })))
}
