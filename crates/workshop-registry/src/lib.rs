//! workshop-registry - the keystone of the workshop server
//! decomposition: one proxy slot per subsystem, into which subsystems
//! self-register their routes, state handles, background tasks, push
//! channels, and shutdown hooks. Consumers reach subsystems through the
//! registry instead of by name, so the composition root never hand-wires
//! what a subsystem can announce itself.
//!
//! ## Invariants
//!
//! - Tier: vocabulary; may depend on: `workshop-protocol` (the wire
//!   types the push-channel slots carry). Read `AGENTS.md` before adding
//!   an import.
//! - Every file in this crate stays under 500 lines; split first, then
//!   edit.
//! - Every subsystem trait is sealed (a private empty supertrait), so
//!   only this crate implements them: registrants plug in through the
//!   adapters provided here, never by implementing a trait downstream.
//! - An unregistered slot is a graceful no-op, never an error:
//!   consumers branch on `None` and continue degraded, and the [`Push`]
//!   facade drops intents whose sink slot is empty.
//! - A registration is alive exactly as long as its guard: dropping the
//!   guard deregisters the subsystem.

mod push;
mod registry;
mod slot;
mod traits;

pub use push::{MenuPush, Push};
pub use registry::Registry;
pub use slot::{ProxySlot, Registration};
pub use traits::{
    BackgroundTasks, CatalogSink, CatalogSinkAdapter, MenuSink, MenuSinkAdapter, RouteRegistrar,
    RouteRegistrarAdapter, ShutdownHook, StateProvider, StateProviderAdapter, StatusChannel,
    StatusChannelAdapter, StatusSink, StatusSinkAdapter, WorkspaceRoots, WorkspaceRootsAdapter,
};
