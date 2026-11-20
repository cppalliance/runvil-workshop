//! The Host state a conversation's run carries in its setup: the menu's
//! selected model, read through the registry as the run starts. The
//! Harness resolves the run's model from it.

use runvil::RunSetup;
use workshop_menu::MenuHandles;
use workshop_registry::Registry;

/// `setup` with its selected model read from `registry`: `selected_model`
/// from the menu's retained workbench state. An unregistered menu reads
/// as no selection.
pub(super) fn with_host_state(registry: &Registry, setup: RunSetup) -> RunSetup {
    let selected_model = registry
        .state::<MenuHandles>()
        .and_then(|handles| handles.menu().latest())
        .and_then(|snapshot| snapshot.selected_model);
    RunSetup {
        selected_model,
        ..setup
    }
}

#[cfg(test)]
#[path = "bindings-tests.rs"]
mod tests;
