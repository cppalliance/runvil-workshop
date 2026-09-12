//! workshop-menu - the server-owned Model menu subsystem: the workbench
//! snapshot pushed to every `/ws` session as a `{"type":"workbench",...}`
//! frame, the chat-capable model catalog channel rebroadcast as
//! `{"type":"models",...}` frames, and the per-profile model memory
//! persisted in the state directory.
//!
//! ## Invariants
//!
//! - Tier: service; may depend on: `workshop-protocol`, `workshop-registry`,
//!   `workshop-support`. Read `AGENTS.md` before adding an import.
//! - Every file in this crate stays under 500 lines; split first, then
//!   edit.
//! - The server owns all Model-menu state and the UI only renders it;
//!   `chat_ready` is computed here and never derived client-side.
//! - Publishing never blocks: a publish with no sessions is a no-op, and
//!   a lagging session skips ahead - every push is a complete snapshot, so
//!   an overwritten one loses nothing. Both buses retain the newest push,
//!   so a session connecting later receives the current state immediately.
//! - Mutation is zone two throughout: a refused mutation is a value
//!   returned to the caller, and a missing, unreadable, or corrupt memory
//!   file means "no memory yet" - logged and tolerated, never fatal.

pub mod catalog;
pub mod menu;

use std::sync::Arc;

pub use catalog::{CatalogBus, ChatCatalog, is_chat_capable};
pub use menu::{MenuBus, MenuRefusal, SwitchOutcome};
use workshop_registry::{
    CatalogSink, CatalogSinkAdapter, MenuSink, MenuSinkAdapter, Registration, Registry,
};

/// Registers the menu subsystem's producer sinks into the registry: the
/// catalog channel's receiving end and the workbench mutators, which
/// same-tier subsystems (the gateway heartbeat's refreshes) drive through
/// the registry's push facade. The returned guards keep the registrations
/// alive; the composition root holds them for the process lifetime.
pub fn register(
    registry: &Registry,
    catalog: &CatalogBus,
    menu: &MenuBus,
) -> (Registration<dyn CatalogSink>, Registration<dyn MenuSink>) {
    let catalog_guard = registry
        .catalog_sink()
        .register(Arc::new(CatalogSinkAdapter::new({
            let catalog = catalog.clone();
            move |models| catalog.publish(models)
        })));
    let menu_guard = registry.menu_sink().register(Arc::new(MenuSinkAdapter::new(
        {
            let menu = menu.clone();
            move |reachable| menu.set_gateway_reachable(reachable)
        },
        {
            let menu = menu.clone();
            move |profiles, active| menu.set_profiles(profiles, active)
        },
        {
            let menu = menu.clone();
            move || menu.restore_selection()
        },
        {
            let menu = menu.clone();
            move || menu.reconcile_catalog()
        },
    )));
    (catalog_guard, menu_guard)
}
