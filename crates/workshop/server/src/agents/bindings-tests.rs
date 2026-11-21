//! Host binding tests: a run's setup takes the menu's selection.

use std::sync::Arc;

use harness_gateway_client::TokioTimer;
use runvil::harness::{MemoryRecorder, OfflineBroker};
use workshop_menu::{CatalogBus, MenuBus};

use super::*;

/// A setup with nothing selected.
fn empty_setup() -> RunSetup {
    RunSetup::new(
        Arc::new(MemoryRecorder::new()),
        Arc::new(OfflineBroker),
        Arc::new(TokioTimer),
    )
}

#[test]
fn a_runs_setup_takes_the_menus_selection() {
    let registry = Registry::new();
    let empty = with_host_state(&registry, empty_setup());
    assert_eq!(
        empty.selected_model, None,
        "an unregistered menu serves no selection"
    );

    let catalog = CatalogBus::new();
    let menu = MenuBus::new(catalog.clone(), None);
    let _menu_guards = workshop_menu::register(&registry, &catalog, &menu);
    let registered = with_host_state(&registry, empty_setup());
    assert_eq!(
        registered.selected_model, None,
        "a registered menu with nothing selected still serves no selection"
    );

    catalog.publish(vec![serde_json::json!({ "id": "test-model" })]);
    menu.set_selected("test-model")
        .expect("the id is in the catalog");

    let setup = with_host_state(&registry, empty_setup());
    assert_eq!(setup.selected_model.as_deref(), Some("test-model"));
}
