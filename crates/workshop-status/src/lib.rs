//! workshop-status - the status-bar subsystem: a broadcast bus carrying
//! status updates from every subsystem to every connected `/ws` session,
//! and the renderer task that turns the process progress hub's snapshots
//! into the status bar's progress indicator.
//!
//! ## Invariants
//!
//! - Tier: service; may depend on: `workshop-protocol`, `workshop-registry`,
//!   `workshop-support`. Read `AGENTS.md` before adding an import.
//! - Every file in this crate stays under 500 lines; split first, then
//!   edit.
//! - Sending on the bus never blocks: a send with no subscribers is a
//!   no-op, and a lagging subscriber skips ahead, so instrumenting a hot
//!   path cannot stall the subsystem it observes.
//! - The bus retains the newest update, so a session that connects later
//!   sends the current status immediately - the delivery contract's
//!   resend-on-reconnect for ephemeral frames.
//! - The public API is infallible (sends are no-ops on lag or empty
//!   rings, never errors), so the crate carries no thiserror error type -
//!   the same exemption `workshop-registry` takes.

pub mod status;

pub mod progress;

use std::sync::Arc;

pub use status::StatusBus;
use workshop_registry::{
    Registration, Registry, StateProvider, StateProviderAdapter, StatusChannel,
    StatusChannelAdapter, StatusSink, StatusSinkAdapter,
};

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
