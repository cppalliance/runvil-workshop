//! The agent-sessions subsystem of the server: the `/agents/ws`
//! agent-session socket ([`socket`]) and its frames ([`wire`]), the
//! `/v1/models` catalog relay ([`relay`]), their shared route state
//! ([`state`]), and [`AgentSessions`], the server's launcher of agent
//! conversations. The `/ws` workshop socket sits outside this subsystem;
//! the two share only [`crate::websocket`].
//!
//! The server builds one [`Host`] at startup, through its builder: a
//! finder that reads each agent from the agents folder through
//! `load_agent`, and its Plugins in the order they install. The linked
//! mcp Plugin is first, over the servers of the configured `mcp.json`.
//! The library Plugins in the configured Plugin directory follow, the
//! std Plugin with the file-tool Pack, the bashkit Plugin with the shell
//! Pack, and the web Plugin with the web Pack among them. Every Host has
//! the built-ins subagents and user beside their Packs. Every tool call
//! of a
//! library Plugin is lent the server's [`resources`]: the Gateway the
//! registry holds as the call starts, which is where the web Pack sends a
//! search. Each launch opens a conversation in `workshop-agents` and builds
//! that conversation's one run its own [`RunSetup`]: the conversation's
//! recorder tee on a handle of the run log's [`TursoRecorder`] that names
//! the launched agent, the server's inference broker ([`broker`]), the
//! stock tokio timer of the Gateway client, a user, the conversation's
//! operator, so the run holds
//! `runvil/user`; the operator's messages reach the run's main chain
//! through the conversation, and a streaming callback that publishes each
//! live piece of a section's own round into the conversation under the
//! round's id. The run makes its own instance of each Pack
//! it holds and drops them when it ends. The run's model rounds reach the
//! gateway only through the server's inference broker, which follows the
//! live gateway binding and sends each round to the dropdown's current
//! pick. Each
//! run's filesystem is an in-memory root, with a read-only mount beside it
//! for each of two demos whose switch is on: the language guide, and the
//! machine's C drive (`language_guide`). Both switches are committed as
//! off. The filesystem has no policy of its own, so the default one,
//! `AllowAll`, admits every operation: the file tools and the shell change
//! the files of the in-memory root, which end with the run, and a
//! read-only mount refuses each change. The run's setup takes the Host
//! state ([`bindings`]): the menu's pick, read once the menu's catalog
//! holds a chat-capable model.
//! Status-bar reporting stays in the server (`status`): a
//! per-conversation reporter derives it from the conversation's events,
//! deltas, and failure reports.
//!
//! **Registry carve-out.** Conversations survive socket disconnect and
//! sockets attach and detach (`socket`), so the launcher keeps the
//! conversation table the server's socket rule otherwise forbids. The rule
//! governed per-request relay work, where every held resource belonged to
//! one socket; a conversation is longer-lived than any socket on purpose.

mod bindings;
mod broker;
mod gateway;
mod language_guide;
mod relay;
mod socket;
mod socket_frames;
mod state;
mod status;
mod wire;

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use harness_gateway_client::{
    GatewayEndpoint, SecretString, TokioTimer, provide_gateway_resources,
};
use runvil::effect::Round;
use runvil::harness::{HostResources, RunId, insert_plugin_dir};
use runvil::plugin::PackRegistry;
use runvil::{Error, ErrorCode, ErrorFact, Host, PromptFinder, RunSetup};
use serde_json::Value;
use workshop_agents::{
    Conversation, ConversationId, Conversations, DeltaKind, LaunchError, discover_agents,
    load_agent,
};
use workshop_gateway::GatewayHandles;
use workshop_registry::Registry;
use workshop_run_log::TursoRecorder;
use workshop_support::{Config, ReconnectBackoff};

use bindings::with_host_state;
use broker::WorkshopBroker;
use gateway::{UsableGateway, usable_gateway};
use socket::Receivers;
pub(crate) use state::{SessionsState, register};

/// The directory under the server's state directory the run log sits in:
/// the `runs.db` every conversation's run is recorded in.
const HARNESS_STATE_DIR: &str = "harness";

/// The server's Host, which finds its agents through `prompts`.
/// `registry` holds the Gateway that each tool call of a library Plugin
/// is lent ([`resources`]). `mcp` is the `[agents] mcp` setting: the mcp
/// Pack is installed over the servers of the file it names. `plugins` is
/// the `[agents] plugins` setting: the directory whose library Plugins
/// are loaded.
fn server_host(
    prompts: Arc<dyn PromptFinder>,
    registry: &Registry,
    mcp: Option<&Path>,
    plugins: Option<&Path>,
) -> Arc<Host> {
    with_plugins(prompts, resources(registry), read_mcp_servers(mcp), plugins)
}

/// The `mcpServers` object of the `mcp.json`-format file at `setting`,
/// with every other top-level key ignored. No setting or a missing file
/// gives an empty object, and so does a file that cannot be read or is
/// not an `mcpServers` object, with a warning that names the file and the
/// rule and holds no entry value. The object is handed on as it stands:
/// the mcp Pack refuses a malformed entry on its own, and the other
/// servers still connect.
fn read_mcp_servers(setting: Option<&Path>) -> Value {
    let none = || Value::Object(serde_json::Map::new());
    let Some(path) = setting else {
        return none();
    };
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return none(),
        Err(error) => {
            tracing::warn!(
                path = %path.display(),
                %error,
                "the MCP file could not be read; no MCP servers are configured"
            );
            return none();
        }
    };
    let document = match serde_json::from_str::<Value>(&text) {
        Ok(document) => document,
        Err(error) => {
            tracing::warn!(
                path = %path.display(),
                %error,
                "the MCP file is not valid JSON; no MCP servers are configured"
            );
            return none();
        }
    };
    let Value::Object(mut top) = document else {
        tracing::warn!(
            path = %path.display(),
            "the MCP file must be a JSON object; no MCP servers are configured"
        );
        return none();
    };
    match top.remove("mcpServers") {
        None => none(),
        Some(servers @ Value::Object(_)) => servers,
        Some(_) => {
            tracing::warn!(
                path = %path.display(),
                "the MCP file's `mcpServers` must be a JSON object; no MCP servers are configured"
            );
            none()
        }
    }
}

/// The server's Host over its Plugins, which finds its agents through
/// `prompts`.
///
/// The linked mcp Plugin installs first, over the `servers` object, with
/// the handle of the runtime this is called on. It is installed only on a
/// runtime: called outside one, as a synchronous test calls it, the Host
/// is built without the mcp Pack and refuses a prompt that requires
/// `mcp`. Then every library Plugin in the directory `plugins` loads,
/// when the server is given one, and every tool call of its Packs is lent
/// `resources`. A directory that does not exist is a state, not a
/// failure: a build that has not placed its Plugins has none, so the Host
/// is built without that directory. The std Plugin, whose Pack holds the
/// file tools, the
/// bashkit Plugin, whose Pack holds the shell, and the web Plugin, whose
/// Pack holds the web tools, are three of those libraries, so a Host
/// whose Plugin directory lacks one has no `std`, no `bash`, or no `web`
/// Pack and refuses a prompt that requires it. A `tracing` warning says
/// so at startup. Every Host also has the built-ins `subagents` and
/// `user`, and each run makes its own instances of the Packs it holds.
///
/// The Host keeps one report for each Plugin step: the mcp Plugin's,
/// when it is installed, and then the Plugin directory's, which lists
/// each library Plugin that loaded and each failure. The Harness logs
/// each of them through `tracing` as it builds the Host.
fn with_plugins(
    prompts: Arc<dyn PromptFinder>,
    resources: HostResources,
    servers: Value,
    plugins: Option<&Path>,
) -> Arc<Host> {
    let mut builder = Host::builder(prompts).with_resources(resources);
    if let Ok(runtime) = tokio::runtime::Handle::try_current() {
        builder =
            builder.with_rust_plugin(move |packs| plugin_mcp::install(packs, servers, runtime));
    }
    // A directory that is known to be missing is a state: a build that has
    // not placed its Plugins has none.
    if let Some(directory) = plugins.filter(|directory| directory.try_exists().unwrap_or(true)) {
        builder = builder.with_plugin_dir(directory);
    }
    let host = builder.build();
    for pack in SHIPPED_PACKS {
        if !holds(&host, pack) {
            tracing::warn!(
                directory = ?plugins,
                pack,
                "no library Plugin inserted a Pack that Workshop ships; an agent that requires it is refused"
            );
        }
    }
    host
}

/// The Packs that only a library Plugin Workshop ships inserts: the file
/// tools, from the std Plugin, the shell, from the bashkit Plugin, and the
/// web tools, from the web Plugin.
const SHIPPED_PACKS: [&str; 3] = ["runvil/std", "runvil/bash", "runvil/web"];

/// Whether `host` has the Pack named `pack`, a vendor and a Pack id.
fn holds(host: &Host, pack: &str) -> bool {
    host.packs().any(|info| info.fqn == pack)
}

/// What the library Plugins in one directory do when they are loaded.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct PluginListing {
    /// Each Plugin that loads, as its name, a space, and its version, in
    /// the order the directory loads them.
    pub loaded: Vec<String>,
    /// Each library that does not load, as its path and the reason.
    pub failed: Vec<String>,
}

/// Loads every library Plugin in `directory`, lists what each did, and
/// unloads them all again.
///
/// The desktop app prints the listing for `runvil-workshop --plugins`.
/// Workshop keeps no log, so this is how a person, or an installer test,
/// sees which Plugins an installation loads.
///
/// # Errors
/// Returns the I/O error when `directory` cannot be read, as when it does
/// not exist.
pub fn list_plugin_dir(directory: &Path) -> io::Result<PluginListing> {
    let mut packs = PackRegistry::new();
    // A listing only loads and unloads the Plugins, so it offers them
    // empty resources.
    let report = insert_plugin_dir(&mut packs, directory, &HostResources::new())?;
    let loaded = report
        .plugins()
        .iter()
        .map(|plugin| format!("{} {}", plugin.name(), plugin.version()))
        .collect();
    let failed = report
        .failures()
        .iter()
        .map(|failure| {
            let path = failure
                .facts
                .get(&ErrorFact::Path)
                .and_then(Value::as_str)
                .unwrap_or_default();
            format!("{path}: {failure}")
        })
        .collect();
    Ok(PluginListing { loaded, failed })
}

/// What the server offers every library Plugin it loads: the Gateway that
/// `registry` holds. The Harness asks for it as each tool call of such a
/// Plugin starts, so a replaced Gateway serves the next call, and a call
/// that starts while the heartbeat reports the Gateway down is lent
/// resources that leave it out. The web Pack sends a search to the
/// Gateway its call is lent.
fn resources(registry: &Registry) -> HostResources {
    let mut resources = HostResources::new();
    let registry = registry.clone();
    // The first offer on empty resources, so the Gateway's name is free
    // and the call cannot be refused.
    let _ = provide_gateway_resources(&mut resources, move || reachable_gateway(&registry));
    resources
}

/// The Gateway `registry` holds now, when the heartbeat says it is
/// reachable: its API root and its bearer key. `None` when no gateway
/// handles are registered, when the heartbeat reports the Gateway down,
/// and when the current generation's URL or key cannot build.
fn reachable_gateway(registry: &Registry) -> Option<(GatewayEndpoint, SecretString)> {
    let handles = registry.state::<GatewayHandles>()?;
    if !handles.health().is_reachable() {
        return None;
    }
    let gateway = UsableGateway::read(&handles)?;
    Some((gateway.endpoint, gateway.key))
}

/// The Host finder over the agents folder at `agents_path`: the source
/// `load_agent` reads for each name, or its refusal as [`agent_error`].
/// It comes inside an `Arc`, as a Host's builder takes its finder.
fn agent_finder(agents_path: PathBuf) -> Arc<dyn PromptFinder> {
    Arc::new(move |name: &str| {
        load_agent(&agents_path, name)
            .map(|(_file, source)| source)
            .map_err(|refusal| agent_error(&refusal))
    })
}

/// The Host finder's failure for an agent `load_agent` refused: the
/// refusal's display text, under `NotFound` for an agent discovery does
/// not list and `Io` for one whose source cannot be read.
fn agent_error(refusal: &LaunchError) -> Error {
    let code = match refusal {
        LaunchError::UnknownAgent { .. } => ErrorCode::NotFound,
        _ => ErrorCode::Io,
    };
    Error::new(code, refusal.to_string(), code.prose())
}

/// The server's launcher of agent conversations: discovery, launch,
/// lookup, and close, plus the server-side work a launch wires up - the
/// run's setup and the status reporter.
///
/// Typed and construction-phased: the registry, the server's backoff, the
/// run log, and the Host every run shares are captured when
/// the composition root builds it, and the other subsystems are read
/// through the registry at the point of use, so this handle never holds
/// another subsystem's handle.
#[derive(Clone)]
pub struct AgentSessions {
    inner: Arc<Inner>,
}

/// The shared state behind the cloneable handle.
struct Inner {
    /// The subsystem registry: the gateway and menu handles and the push
    /// facade are read through it.
    registry: Registry,
    /// Reset on completed replies: an agent reply is useful gateway work.
    backoff: ReconnectBackoff,
    /// The folder the launchable agents are discovered in.
    agents_path: PathBuf,
    /// The run log every run is recorded in, through a handle naming its
    /// agent behind each run's tee.
    recorder: Arc<TursoRecorder>,
    /// The inference broker every run's setup holds.
    broker: WorkshopBroker,
    /// What every run shares: the installed Packs each run makes its
    /// instances of, the built-ins among them, and the finder that reads
    /// each agent from the agents folder. It also keeps each library
    /// Plugin it loaded, so one that inserted no Pack still stays loaded
    /// for as long as the launcher lives.
    host: Arc<Host>,
    /// The running conversations.
    conversations: Conversations,
}

impl fmt::Debug for AgentSessions {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentSessions")
            .field("conversations", &self.inner.conversations)
            .finish_non_exhaustive()
    }
}

impl AgentSessions {
    /// Builds the launcher for `config` over the subsystem registry and
    /// the server's reconnect backoff. The only file this reads is the
    /// `[agents] mcp` file, when the setting names one, and the only
    /// connections it starts are the background ones to that file's MCP
    /// servers, on the server's runtime. It opens each dynamic library in
    /// the `[agents] plugins` directory, when the setting names one, and
    /// loads its Plugin, which runs that library's code. Nothing else is
    /// spawned and nothing else touches the filesystem here: the run log
    /// opens under the state directory when the first run starts.
    #[must_use]
    pub fn new(config: &Config, registry: Registry, backoff: ReconnectBackoff) -> Self {
        let agents_path = config.agents.path.clone();
        let host = server_host(
            agent_finder(agents_path.clone()),
            &registry,
            config.agents.mcp.as_deref(),
            config.agents.plugins.as_deref(),
        );
        Self {
            inner: Arc::new(Inner {
                agents_path,
                recorder: Arc::new(TursoRecorder::new(
                    config.server.state_dir.join(HARNESS_STATE_DIR),
                )),
                broker: WorkshopBroker::new(registry.clone()),
                host,
                conversations: Conversations::new(),
                registry,
                backoff,
            }),
        }
    }

    /// The launchable agent names: the `.md` file stems under the
    /// configured agents directory plus the built-in `chat`, sorted.
    #[must_use]
    pub fn discover(&self) -> Vec<String> {
        discover_agents(&self.inner.agents_path)
    }

    /// Launches a conversation running the discovered agent `name` and
    /// returns it with its channels, subscribed before the run starts so
    /// the launching socket sees everything the run reports, a parse
    /// failure included. Its run goes until its program returns, fails,
    /// or [`close`](Self::close) ends it; a stop drops only the round in
    /// flight.
    ///
    /// A launch with no usable gateway is refused before the agent is
    /// read, since its every model round would fail.
    ///
    /// # Errors
    /// Returns [`LaunchRefusal::GatewayUnusable`] when no gateway is
    /// registered or its URL or key cannot build, and the conversation's
    /// own [`LaunchError`] otherwise: an unknown agent or an unreadable
    /// agent source.
    async fn launch(&self, name: &str) -> Result<(Conversation, Receivers), LaunchRefusal> {
        if usable_gateway(&self.inner.registry).is_none() {
            return Err(LaunchRefusal::GatewayUnusable);
        }
        // The folder walk and the source read are filesystem work, so
        // they run on the blocking pool; a worker that cannot report is
        // the same unreadable state as an unreadable file. The run reads
        // the agent again through the Host's finder when it parses it.
        let agents_path = self.inner.agents_path.clone();
        let agent = name.to_owned();
        tokio::task::spawn_blocking(move || load_agent(&agents_path, &agent))
            .await
            .unwrap_or_else(|join| {
                Err(LaunchError::Unreadable {
                    source: io::Error::other(join),
                })
            })?;
        let conversation = self.inner.conversations.open(name);
        let receivers = Receivers::subscribe(&conversation);
        status::spawn_reporter(
            &conversation,
            self.inner.registry.push(),
            self.inner.backoff.clone(),
        );
        tokio::spawn(self.clone().drive(conversation.clone()));
        Ok((conversation, receivers))
    }

    /// Runs `conversation`'s one run of its agent. The setup's selected
    /// model is read once the menu's catalog holds a chat-capable model,
    /// so a pick made while the catalog was empty is the one the run
    /// binds; a close during that wait ends the run before it begins.
    async fn drive(self, conversation: Conversation) {
        tokio::select! {
            () = conversation.closing() => {}
            () = self.inner.broker.chat_model_ready() => {}
        }
        let setup = with_host_state(&self.inner.registry, self.setup_for(&conversation));
        conversation
            .run(Arc::clone(&self.inner.host), conversation.agent(), setup)
            .await;
    }

    /// The setup of `conversation`'s run before the Host state is read:
    /// its recorder tee over a run-log handle naming its agent, the
    /// server's inference broker, the stock tokio timer, its filesystem, a
    /// user, since every conversation has an operator, and the streaming
    /// callback that publishes each live piece into the conversation.
    fn setup_for(&self, conversation: &Conversation) -> RunSetup {
        let recorder = conversation.recorder(self.inner.recorder.for_agent(conversation.agent()));
        // The broker is a handle, and the setup owns one. The callback is
        // `'static`, and the caller still needs the conversation to run
        // the run, so the callback takes a handle to it.
        RunSetup::new(recorder, self.inner.broker.clone(), TokioTimer)
            .vfs(language_guide::run_vfs())
            .has_user(true)
            .streaming(live_pieces(conversation.clone()))
    }

    /// The running conversation with this id, when one exists: how a
    /// socket reattaches after a disconnect.
    fn get(&self, id: &str) -> Option<Conversation> {
        self.inner.conversations.get(&ConversationId::new(id))
    }

    /// Ends the conversation with this id: its run is canceled, and the
    /// conversation leaves the table at once. Returns whether a
    /// conversation was ended.
    #[must_use]
    pub fn close(&self, id: &str) -> bool {
        self.inner.conversations.close(&ConversationId::new(id))
    }

    /// The one run of the running conversation with this id, as the run
    /// log issued it; `None` when no such conversation is running or its
    /// run has not begun.
    #[must_use]
    pub fn run_id(&self, id: &str) -> Option<RunId> {
        self.get(id)?.run_id()
    }
}

/// The run's streaming callback: publishes each live piece of a section's
/// own round into `conversation`, under the round's id. It sends on the
/// conversation's own broadcast and returns, and it calls nothing of the
/// Harness.
fn live_pieces(
    conversation: Conversation,
) -> impl Fn(Round, runvil::harness::Delta) + Send + Sync + 'static {
    move |round, piece| {
        let (kind, content) = match piece {
            runvil::harness::Delta::Text(text) => (DeltaKind::Text, text),
            runvil::harness::Delta::Reasoning(text) => (DeltaKind::Reasoning, text),
            // The enum is non-exhaustive across the crate seam; any other
            // piece has no delta kind and stays unshown.
            _ => return,
        };
        conversation.publish_delta(round.id, kind, content);
    }
}

/// A refused agent launch, relayed to the client as an error frame.
#[derive(Debug, thiserror::Error)]
enum LaunchRefusal {
    /// No gateway is registered, or the registered gateway's URL or key
    /// cannot build, so no agent could complete a model round.
    #[error("agent sessions need a usable gateway binding; check the gateway base URL and key")]
    GatewayUnusable,
    /// The agent was refused.
    #[error(transparent)]
    Refused(#[from] LaunchError),
}

#[cfg(test)]
mod tests;
