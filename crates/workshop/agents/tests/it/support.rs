//! Shared fixtures: a Host and a per-run setup built around the recorder
//! tee a conversation hands it.

use std::sync::Arc;

use harness_gateway_client::TokioTimer;
use runvil::harness::{FindTextPrompt, MemoryRecorder, OfflineBroker};
use runvil::{Host, RunSetup};
use workshop_agents::Conversation;

/// The name the fixture Host finds its one agent by.
pub(crate) const AGENT: &str = "fixture";

/// A prompt whose one section runs `lua`, declaring the user Pack, as a
/// conversation's agent does.
pub(crate) fn prompt(lua: &str) -> String {
    format!(
        "---\nname: fixture\ndescription: a conversation fixture\nrunvil: 0\n\
         uses:\n  - user\n---\n\n\
         ## Conversation\n\n```lua\n{lua}\n```\n"
    )
}

/// A Host that finds `source` as [`AGENT`], beside the setup of a run with
/// a user, recording into `recorder` through the conversation's tee.
pub(crate) fn run_for(
    conversation: &Conversation,
    recorder: Arc<MemoryRecorder>,
    source: String,
) -> (Arc<Host>, RunSetup) {
    let host = Host::builder(FindTextPrompt::new(AGENT, source)).build();
    let setup =
        RunSetup::new(conversation.recorder(recorder), OfflineBroker, TokioTimer).has_user(true);
    (host, setup)
}
