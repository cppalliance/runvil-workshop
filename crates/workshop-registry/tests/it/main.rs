//! Integration tests for `workshop-registry`: the proxy-slot contract.

use std::sync::{Arc, Mutex, PoisonError};

use tokio::sync::broadcast;

use workshop_protocol::{Activity, Severity, StatusBarUpdate};
use workshop_registry::{Registry, StatusChannelAdapter};

/// The adapter's boxed subscribe closure type.
type Subscribe = Box<dyn Fn() -> broadcast::Receiver<StatusBarUpdate> + Send + Sync>;
/// The adapter's boxed latest-snapshot closure type.
type Latest = Box<dyn Fn() -> Option<StatusBarUpdate> + Send + Sync>;

/// A hand-rolled status bus plus its registration adapter: a broadcast
/// sender and a retained snapshot, mirroring the registrant's real bus.
struct TestBus {
    adapter: StatusChannelAdapter<Subscribe, Latest>,
    sender: broadcast::Sender<StatusBarUpdate>,
    latest: Arc<Mutex<Option<StatusBarUpdate>>>,
}

/// Builds a test bus with a fresh channel and no snapshot.
fn status_adapter() -> TestBus {
    let (sender, _) = broadcast::channel(4);
    let latest = Arc::new(Mutex::new(None));
    let subscribe: Subscribe = Box::new({
        let sender = sender.clone();
        move || sender.subscribe()
    });
    let latest_snapshot: Latest = Box::new({
        let latest = Arc::clone(&latest);
        move || {
            latest
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone()
        }
    });
    TestBus {
        adapter: StatusChannelAdapter::new(subscribe, latest_snapshot),
        sender,
        latest,
    }
}

/// A status update carrying only a label.
fn update(label: &str) -> StatusBarUpdate {
    StatusBarUpdate {
        label: label.to_string(),
        description: String::new(),
        progress: None,
        severity: Severity::Info,
        activity: Activity::General,
    }
}

#[test]
fn an_unregistered_slot_is_a_graceful_no_op() {
    let registry = Registry::new();
    assert!(registry.status_channel().is_none());
    assert!(registry.routes().get().is_none());
    assert!(registry.state_handles().get().is_none());
    assert!(registry.tasks().get().is_none());
    assert!(registry.shutdown().get().is_none());
}

#[test]
fn a_registered_status_channel_serves_subscribe_and_latest() {
    let registry = Registry::new();
    let bus = status_adapter();
    let _registration = registry.status().register(Arc::new(bus.adapter));
    let channel = registry
        .status_channel()
        .expect("the registered channel is served");
    *bus.latest.lock().unwrap_or_else(PoisonError::into_inner) = Some(update("Ready"));
    assert_eq!(
        channel.latest().map(|update| update.label),
        Some("Ready".to_string()),
        "the slot serves the registrant's retained snapshot"
    );
    let mut receiver = channel.subscribe();
    bus.sender
        .send(update("Working"))
        .expect("a receiver is subscribed");
    assert_eq!(
        receiver
            .try_recv()
            .expect("the send reaches the subscriber")
            .label,
        "Working"
    );
}

#[test]
fn dropping_the_guard_deregisters_the_subsystem() {
    let registry = Registry::new();
    let bus = status_adapter();
    let registration = registry.status().register(Arc::new(bus.adapter));
    assert!(registry.status_channel().is_some());
    drop(registration);
    assert!(
        registry.status_channel().is_none(),
        "the slot empties when the guard drops"
    );
}

#[test]
fn a_stale_guard_never_evicts_a_newer_occupant() {
    let registry = Registry::new();
    let stale = registry
        .status()
        .register(Arc::new(status_adapter().adapter));
    let _current = registry
        .status()
        .register(Arc::new(status_adapter().adapter));
    drop(stale);
    assert!(
        registry.status_channel().is_some(),
        "the replacement survives the stale guard's drop"
    );
}

#[test]
fn registry_clones_share_the_same_slots() {
    let registry = Registry::new();
    let clone = registry.clone();
    let _registration = registry
        .status()
        .register(Arc::new(status_adapter().adapter));
    assert!(
        clone.status_channel().is_some(),
        "a registration through one handle is visible through every clone"
    );
}
