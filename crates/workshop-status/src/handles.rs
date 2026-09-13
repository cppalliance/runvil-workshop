//! The status subsystem's registration: the consumer-side push channel
//! every `/ws` session subscribes through, the producer-side sink
//! same-tier subsystems emit through, and the bus itself as the
//! subsystem's state handle.

use std::sync::Arc;

use workshop_registry::{
    Registration, Registry, StateProvider, StateProviderAdapter, StatusChannel,
    StatusChannelAdapter, StatusSink, StatusSinkAdapter,
};

use crate::StatusBus;

/// Registers the status subsystem into the registry: the consumer-side
/// push channel every `/ws` session subscribes through, the
/// producer-side sink same-tier subsystems emit through, and the bus
/// itself as the subsystem's state handle. The returned guards keep the
/// registrations alive; the composition root holds them for the process
/// lifetime.
pub fn register(
    registry: &Registry,
    bus: &StatusBus,
) -> (
    Registration<dyn StatusChannel>,
    Registration<dyn StatusSink>,
    Registration<dyn StateProvider>,
) {
    let channel = registry
        .status()
        .register(Arc::new(StatusChannelAdapter::new(
            {
                let bus = bus.clone();
                move || bus.subscribe()
            },
            {
                let bus = bus.clone();
                move || bus.latest()
            },
        )));
    let sink = registry
        .status_sink()
        .register(Arc::new(StatusSinkAdapter::new({
            let bus = bus.clone();
            move |update| bus.emit(update)
        })));
    let state = registry
        .status_state()
        .register(Arc::new(StateProviderAdapter::new({
            let bus = bus.clone();
            move || Arc::new(bus.clone()) as Arc<dyn std::any::Any + Send + Sync>
        })));
    (channel, sink, state)
}
