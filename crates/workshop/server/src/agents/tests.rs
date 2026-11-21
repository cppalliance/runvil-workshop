//! The agent conversations' Harness setup: after the server installs the
//! linked mcp Plugin, loads the std and web Plugins' libraries from its
//! Plugin directory, and builds its Host, every run of a conversation,
//! which has a user, holds the `web`, `subagents`, `mcp`, `std`, and
//! `user` Packs. A prompt declaring `web` prepares over a Plugin
//! directory that holds the web library, and one declaring the built-in
//! `subagents` prepares over the server's installed Packs. The `mcp.json`
//! reader returns the `mcpServers` object as it stands, or an empty one.
//! A prompt declaring `mcp` prepares on a Host built on a runtime, whose
//! handle the mcp Pack is installed with, and a Host built outside a
//! runtime installs no mcp Pack and refuses that prompt. The servers
//! value is the mcp Pack's server list, so one it refuses refuses a
//! prompt requiring `mcp` as it parses. The Host's finder refuses an
//! unknown agent as `NotFound` and an unreadable one as `Io`, each with
//! the launch refusal's own text, and it reads an agent that another
//! names under `prompts:`, which that agent then runs with `execute`. A
//! configured Plugin directory loads each library Plugin in it and
//! reports each file that is none, a directory that does not exist
//! loads nothing, and one that cannot be read is a failure of the Host's
//! report while the Host is built. A prompt reads and changes the files
//! of its run through
//! the std library's `Read`, `Write`, `StrReplace`, and `Delete` tools
//! and through the bashkit library's `Shell` tool, and the Host reads
//! each change back after the run. A Host with no Plugin directory
//! refuses a prompt that requires `std`, `bash`, or `web`. The Gateway
//! that a tool call of a library Plugin is lent has its tests in
//! `gateway`.

use std::env::consts::{DLL_EXTENSION, DLL_PREFIX, DLL_SUFFIX};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use harness_gateway_client::TokioTimer;
use runvil::harness::{MemoryRecorder, OfflineBroker, PluginReport, PluginSource};
use runvil::vfs::{Origin, VfsRef};
use runvil::{Error, ErrorCode, ErrorFact, Host, PromptFinder, RunSetup};
use serde_json::json;
use workshop_agents::{Conversations, SessionState, load_agent};
use workshop_registry::Registry;

use super::{
    PluginListing, SHIPPED_PACKS, agent_finder, holds, list_plugin_dir, read_mcp_servers,
    resources, server_host, with_plugins,
};

#[path = "tests-gateway.rs"]
mod gateway;

/// A prompt that requires the Pack `pack` and returns a fixed text.
fn requiring(pack: &str) -> String {
    format!(
        "---\nname: browses\ndescription: needs {pack}\nrunvil: 0\n\
        uses:\n  - {pack}\n---\n\n\
        ## Only\n\n```lua\nreturn 'browsed'\n```\n"
    )
}

/// A finder that gives `source` under every name.
pub(super) fn serving(source: impl Into<String>) -> Arc<dyn PromptFinder> {
    let source = source.into();
    Arc::new(move |_: &str| -> Result<String, Error> { Ok(source.clone()) })
}

/// The report of the Plugin directory of `host`, which is the last step
/// its builder names.
fn directory_report(host: &Host) -> &PluginReport {
    host.plugin_reports()
        .last()
        .expect("the Host names a Plugin directory")
}

/// The name of each Plugin that the Plugin directory of `host` loaded.
fn loaded(host: &Host) -> Vec<&str> {
    directory_report(host)
        .plugins()
        .iter()
        .map(|plugin| plugin.name())
        .collect()
}

/// Asserts that the Plugin directory of `host` reported no failure.
#[track_caller]
fn assert_no_failures(host: &Host) {
    let failures = directory_report(host).failures();
    assert!(failures.is_empty(), "{failures:?}");
}

/// Runs the prompt that `host` finds under every name as one
/// conversation over the default in-memory filesystem; see
/// [`run_agent_on`].
async fn run_prompt(host: Arc<Host>) -> Result<Result<String, Error>, String> {
    run_prompt_over(host, VfsRef::default()).await
}

/// Runs the prompt that `host` finds under every name as one
/// conversation over the filesystem `vfs`; see [`run_agent_on`].
pub(super) async fn run_prompt_over(
    host: Arc<Host>,
    vfs: VfsRef,
) -> Result<Result<String, Error>, String> {
    run_agent_on(host, "browses", vfs).await
}

/// Runs the agent `name` of `host` as one conversation, which has a user,
/// over the filesystem `vfs`, then reads the conversation's state and how
/// it ended: the result of its one run, or the message of the run-failed
/// report when the Host refused the prompt as it parsed it and no run
/// began.
async fn run_agent_on(
    host: Arc<Host>,
    name: &str,
    vfs: VfsRef,
) -> Result<Result<String, Error>, String> {
    let recorder = Arc::new(MemoryRecorder::new());
    let conversation = Conversations::new().open(name);
    let mut errors = conversation.subscribe_errors();
    let setup = RunSetup::new(
        conversation.recorder(recorder.clone()),
        Arc::new(OfflineBroker),
        Arc::new(TokioTimer),
    )
    .vfs(vfs)
    .has_user(true);
    tokio::time::timeout(Duration::from_secs(10), conversation.run(host, name, setup))
        .await
        .expect("the run ends in time");
    assert_eq!(
        conversation.state(),
        SessionState::Closed,
        "the conversation ends with its run"
    );
    match conversation.run_id() {
        Some(run) => Ok(recorder.result(run).expect("the recorder ended the run")),
        None => Err(errors
            .try_recv()
            .expect("a prompt refused before its run is reported")
            .message),
    }
}

/// An `mcp.json` file in a new directory holding `text`.
fn mcp_file(text: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::TempDir::new().expect("tempdir");
    let path = dir.path().join("mcp.json");
    std::fs::write(&path, text).expect("write fixture");
    (dir, path)
}

#[tokio::test]
async fn every_run_holds_the_packs_the_three_plugins_add_and_the_builtins() {
    let lists = "---\nname: lists\ndescription: reads its Packs\nrunvil: 0\n---\n\n\
        ## Only\n\n```lua\n\
        local names = {}\n\
        for _, id in ipairs({ 'web', 'subagents', 'mcp', 'std', 'user' }) do\n\
          names[#names + 1] = tools.packs.get(id).name\n\
        end\n\
        return table.concat(names, ',') .. '|' .. tostring(tools.packs.get('user-input'))\n```\n";
    let dir = plugin_dir_with("every-run-holds", &[STD_LIBRARY, WEB_LIBRARY]);
    let host = server_host(serving(lists), &Registry::new(), None, Some(dir.path()));
    assert_eq!(
        run_prompt(host).await,
        Ok(Ok("web,subagents,mcp,std,user|nil".to_owned())),
        "a run that declares nothing holds every runvil Pack the server installed, the \
         built-ins, and the user Pack its user brings, and no user-input Pack"
    );
}

#[tokio::test]
async fn a_prompt_declaring_web_prepares_on_the_servers_built_packs() {
    let (dir, _library) = plugin_dir_of("declares-web", WEB_LIBRARY);
    let host = server_host(
        serving(requiring("web")),
        &Registry::new(),
        None,
        Some(dir.path()),
    );
    assert_no_failures(&host);
    assert_eq!(loaded(&host), ["web"]);
    assert_eq!(
        run_prompt(host).await,
        Ok(Ok("browsed".to_owned())),
        "the web library's Pack is installed, so a prompt that requires web runs"
    );
}

#[tokio::test]
async fn a_prompt_declaring_subagents_prepares_on_the_servers_built_packs() {
    let host = server_host(
        serving(requiring("subagents")),
        &Registry::new(),
        None,
        None,
    );
    assert_eq!(
        run_prompt(host).await,
        Ok(Ok("browsed".to_owned())),
        "the server's Host installs the built-in subagents"
    );
}

#[test]
fn the_finder_refuses_an_unknown_agent_as_not_found_and_an_unreadable_one_as_io() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    // Discovery lists `broken`, but its bytes are not UTF-8, so its read
    // fails.
    std::fs::write(dir.path().join("broken.md"), [0xff, 0xfe]).expect("write fixture");
    let finder = agent_finder(dir.path().to_path_buf());

    for (name, code) in [("missing", ErrorCode::NotFound), ("broken", ErrorCode::Io)] {
        let error = finder.find(name).expect_err("the finder refuses the agent");
        let refusal = load_agent(dir.path(), name).expect_err("the launch check refuses it too");
        assert_eq!(error.code, code, "{name}");
        assert_eq!(error.message, refusal.to_string(), "{name}");
        assert_eq!(error.prose, code.prose(), "{name}");
    }
}

#[tokio::test]
async fn an_agent_executes_an_agent_of_the_folder_that_its_prompts_list_names() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    // A `prompts:` entry is the name the finder knows the agent under,
    // which is the name discovery lists: the file's stem.
    let outer = "---\nname: outer\ndescription: runs inner\nrunvil: 0\n\
        prompts:\n  - inner\n---\n\n## Only\n\n```lua\n\
        return 'outer got ' .. execute('inner', {})\n```\n";
    let inner = "---\nname: inner\ndescription: answers\nrunvil: 0\n---\n\n\
        ## Only\n\n```lua\nreturn 'inner ran'\n```\n";
    std::fs::write(dir.path().join("outer.md"), outer).expect("write fixture");
    std::fs::write(dir.path().join("inner.md"), inner).expect("write fixture");
    let host = server_host(
        agent_finder(dir.path().to_path_buf()),
        &Registry::new(),
        None,
        None,
    );

    assert_eq!(
        run_agent_on(host, "outer", VfsRef::default()).await,
        Ok(Ok("outer got inner ran".to_owned())),
        "the Host's finder reads the listed agent, and `execute` runs it in the same run"
    );
}

#[tokio::test]
async fn a_prompt_declaring_mcp_prepares_on_a_host_built_on_a_runtime() {
    // The Host is built on this test's runtime, as the server builds its
    // own, so the mcp Pack is installed with that runtime's handle, here
    // over no servers.
    let host = server_host(serving(requiring("mcp")), &Registry::new(), None, None);
    assert_eq!(
        run_prompt(host).await,
        Ok(Ok("browsed".to_owned())),
        "a Host built on a runtime installs the mcp Pack once"
    );
}

#[test]
fn a_host_built_outside_a_runtime_installs_no_mcp_pack() {
    // A synchronous test builds the Host outside a runtime, so the Host
    // is built without the mcp Pack. The run itself needs a runtime,
    // which starts only after the Host is built.
    let host = server_host(serving(requiring("mcp")), &Registry::new(), None, None);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a runtime for the run is built");

    let result = runtime.block_on(run_prompt(host));
    let Err(message) = result else {
        panic!("the Host refuses the prompt as it parses: {result:?}");
    };
    assert!(
        message.contains("- mcp is unavailable: runvil/mcp is not installed"),
        "the refusal names the Pack no Plugin inserted: {message}"
    );
}

#[tokio::test]
async fn the_servers_value_is_the_mcp_packs_server_list() {
    // A value that is neither null nor an object is the one server list
    // the install refuses, so only that value reaching it fails the run.
    let host = with_plugins(
        serving(requiring("mcp")),
        resources(&Registry::new()),
        json!([]),
        None,
    );

    let result = run_prompt(host).await;
    let Err(message) = result else {
        panic!("the Host refuses the prompt as it parses: {result:?}");
    };
    assert!(
        message.contains("- mcp is unavailable: the MCP configuration must be a JSON object"),
        "the refusal names the mcp Pack and its server list rule: {message}"
    );
}

/// Counts the Plugin directories this process has made, so two tests
/// never share one.
static PLUGIN_DIRS: AtomicUsize = AtomicUsize::new(0);

/// The std Plugin's library, as Cargo names it.
const STD_LIBRARY: &str = "plugin_std";

/// The bashkit Plugin's library, as Cargo names it.
pub(super) const BASHKIT_LIBRARY: &str = "plugin_bashkit";

/// The web Plugin's library, as Cargo names it.
const WEB_LIBRARY: &str = "plugin_web";

/// The file name of the dynamic library `library`.
fn library_file(library: &str) -> String {
    format!("{DLL_PREFIX}{library}{DLL_SUFFIX}")
}

/// A Plugin directory for the test named `test`, holding a copy of each
/// dynamic library of `libraries`. Cargo builds each library beside this
/// test executable, because the crate depends on the `workshop-plugin-*`
/// crate that exports it. The directory's name comes from the test, the
/// process id, and a counter.
fn plugin_dir_with(test: &str, libraries: &[&str]) -> tempfile::TempDir {
    let prefix = format!(
        "{test}-{}-{}-",
        std::process::id(),
        PLUGIN_DIRS.fetch_add(1, Ordering::Relaxed)
    );
    let dir = tempfile::Builder::new()
        .prefix(&prefix)
        .tempdir()
        .expect("a temporary directory is made");
    let exe = std::env::current_exe().expect("the test executable has a path");
    let built = exe.parent().expect("the test executable has a directory");
    for library in libraries {
        let file = library_file(library);
        std::fs::copy(built.join(&file), dir.path().join(&file))
            .expect("the Plugin's library is copied");
    }
    dir
}

/// A Plugin directory for the test named `test`, holding a copy of the
/// dynamic library `library`, and that copy's path.
pub(super) fn plugin_dir_of(test: &str, library: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = plugin_dir_with(test, &[library]);
    let copy = dir.path().join(library_file(library));
    (dir, copy)
}

/// A Plugin directory for the test named `test`, holding a copy of the
/// std Plugin's library, and that copy's path.
pub(super) fn plugin_dir(test: &str) -> (tempfile::TempDir, PathBuf) {
    plugin_dir_of(test, STD_LIBRARY)
}

#[test]
fn a_plugin_directory_loads_the_std_plugin_from_its_library() {
    let (dir, library) = plugin_dir("loads-std");
    let host = server_host(serving(""), &Registry::new(), None, Some(dir.path()));
    let report = directory_report(&host);

    assert!(report.failures().is_empty(), "{:?}", report.failures());
    let [plugin] = report.plugins() else {
        panic!("one library loads one Plugin: {:?}", report.plugins());
    };
    assert_eq!(plugin.name(), "std");
    assert_eq!(plugin.version(), "0.1.0");
    assert_eq!(
        plugin.source(),
        &PluginSource::Library(std::path::absolute(&library).expect("the path is made absolute"))
    );
}

#[test]
fn a_file_that_is_no_library_is_reported_and_the_std_plugin_still_loads() {
    let (dir, _library) = plugin_dir("reports-a-bad-file");
    let bad = dir.path().join(format!("a-text-file.{DLL_EXTENSION}"));
    std::fs::write(&bad, "not a library").expect("write fixture");
    let host = server_host(serving(""), &Registry::new(), None, Some(dir.path()));
    let report = directory_report(&host);

    assert_eq!(report.plugins().len(), 1, "{:?}", report.plugins());
    let [failure] = report.failures() else {
        panic!("the text file is the one failure: {:?}", report.failures());
    };
    assert_eq!(failure.code, ErrorCode::PluginLoadFailed);
    assert_eq!(failure.facts.get(&ErrorFact::Reason), Some(&json!("open")));
    let expected = std::path::absolute(&bad).expect("the path is made absolute");
    assert_eq!(
        failure.facts.get(&ErrorFact::Path),
        Some(&json!(expected.display().to_string()))
    );
}

#[test]
fn a_listing_names_each_plugin_that_loads_and_each_library_that_does_not() {
    let (dir, _library) = plugin_dir("lists-std");
    assert_eq!(
        list_plugin_dir(dir.path()).expect("the directory is read"),
        PluginListing {
            loaded: vec!["std 0.1.0".to_owned()],
            failed: Vec::new(),
        }
    );

    let bad = dir.path().join(format!("a-text-file.{DLL_EXTENSION}"));
    std::fs::write(&bad, "not a library").expect("write fixture");
    let listing = list_plugin_dir(dir.path()).expect("the directory is read");
    assert_eq!(listing.loaded, ["std 0.1.0"]);
    let [failed] = listing.failed.as_slice() else {
        panic!("the text file is the one failure: {:?}", listing.failed);
    };
    let path = std::path::absolute(&bad).expect("the path is made absolute");
    assert!(
        failed.starts_with(&format!("{}: ", path.display())),
        "the line opens with the library's path: {failed}"
    );

    let missing = list_plugin_dir(&dir.path().join("absent")).expect_err("no such directory");
    assert_eq!(missing.kind(), std::io::ErrorKind::NotFound);
}

#[test]
fn a_plugin_directory_that_does_not_exist_loads_nothing() {
    let (dir, _library) = plugin_dir("missing-directory");
    // A synchronous test builds the Host outside a runtime, so the Plugin
    // directory would be its one step.
    let host = server_host(
        serving(""),
        &Registry::new(),
        None,
        Some(&dir.path().join("absent")),
    );
    assert!(
        host.plugin_reports().is_empty(),
        "a missing directory is a state, not a failure: {:?}",
        host.plugin_reports()
    );
}

#[test]
fn a_plugin_directory_that_cannot_be_read_is_a_failure_and_the_host_is_built() {
    // A regular file in the directory's place exists, so the Host names
    // it, and it cannot be read as a directory. The directory around it
    // is named for this test, as each Plugin directory here is.
    let dir = plugin_dir_with("cannot-be-read", &[]);
    let file = dir.path().join("plugins");
    std::fs::write(&file, "not a directory").expect("write fixture");
    // A synchronous test builds the Host outside a runtime, so the Plugin
    // directory is its one step.
    let host = server_host(serving(""), &Registry::new(), None, Some(&file));

    let [report] = host.plugin_reports() else {
        panic!("the directory is the one step: {:?}", host.plugin_reports());
    };
    assert!(report.plugins().is_empty(), "{:?}", report.plugins());
    let [failure] = report.failures() else {
        panic!("the directory is the one failure: {:?}", report.failures());
    };
    assert_eq!(failure.code, ErrorCode::PluginLoadFailed);
    assert_eq!(
        failure.facts.get(&ErrorFact::Reason),
        Some(&json!("directory"))
    );
    assert_eq!(
        failure.facts.get(&ErrorFact::Path),
        Some(&json!(file.display().to_string())),
        "the failure names the configured path"
    );
    assert!(
        holds(&host, "runvil/user") && !holds(&host, "runvil/std"),
        "the Host is built, with its built-ins and no library Plugin's Pack"
    );
}

#[tokio::test]
async fn the_servers_host_loads_its_plugin_directory_beside_the_linked_plugin() {
    let (dir, _library) = plugin_dir("server-host");
    let host = server_host(
        serving(requiring("std")),
        &Registry::new(),
        None,
        Some(dir.path()),
    );

    assert_eq!(loaded(&host), ["std"]);
    assert_eq!(
        run_prompt(host).await,
        Ok(Ok("browsed".to_owned())),
        "the library's Pack is installed, so a prompt that requires std runs"
    );
}

#[test]
fn only_a_plugin_directory_with_a_shipped_library_gives_the_host_its_pack() {
    let bare = server_host(serving(""), &Registry::new(), None, None);
    for (library, pack) in [
        (STD_LIBRARY, "runvil/std"),
        (BASHKIT_LIBRARY, "runvil/bash"),
        (WEB_LIBRARY, "runvil/web"),
    ] {
        assert!(SHIPPED_PACKS.contains(&pack), "{pack}");
        let (dir, _library) = plugin_dir_of("holds-a-shipped-pack", library);
        let host = server_host(serving(""), &Registry::new(), None, Some(dir.path()));
        assert!(holds(&host, pack), "{library} inserts {pack}");
        assert!(!holds(&bare, pack), "{pack} is not among the built-ins");
    }
}

/// A prompt that declares `std` and returns what four of its tools
/// answer, in this order: `std/read` for `/notes.md`, `std/write` for a
/// file under a directory that is absent, `std/replace` for a text of
/// `/notes.md`, and `std/rm` for `/old.md`.
const READS_AND_CHANGES: &str = "---\nname: changes\ndescription: reads and changes files\n\
    runvil: 0\nuses:\n  - std\n---\n\n## Only\n\n```lua\n\
    local read = tools.call('std/read', { path = '/notes.md' })\n\
    local write = tools.call('std/write', { path = '/out/new.md', contents = 'x' })\n\
    local replace = tools.call('std/replace', {\n\
      path = '/notes.md', old_string = 'world', new_string = 'there',\n\
    })\n\
    local delete = tools.call('std/rm', { path = '/old.md' })\n\
    return read .. '|' .. write .. '|' .. replace .. '|' .. delete\n```\n";

#[tokio::test]
async fn a_prompt_reads_and_changes_files_of_its_run_through_the_std_library_plugin() {
    let (dir, _library) = plugin_dir("reads-and-changes");
    let host = server_host(
        serving(READS_AND_CHANGES),
        &Registry::new(),
        None,
        Some(dir.path()),
    );
    assert_no_failures(&host);

    let vfs = VfsRef::default();
    let seed = vfs
        .acquire(Origin::new("seed"))
        .expect("a fresh memory filesystem acquires");
    seed.write("/notes.md", b"hello\nworld\n")
        .expect("the file is seeded");
    seed.write("/old.md", b"old\n").expect("the file is seeded");

    let result = run_prompt_over(host, vfs.clone()).await;
    let Ok(Ok(text)) = result else {
        panic!("the run ends with its text: {result:?}");
    };
    // Each numbered line holds a `|`, and the three trusted texts hold
    // none, so the answers split from the right.
    let mut answers = text.rsplitn(4, '|');
    let mut next = || answers.next().expect("the prompt joins four answers");
    let (delete, replace, write, read) = (next(), next(), next(), next());
    assert!(
        read.contains("     1|hello\n     2|world\n"),
        "Read answers the file's numbered lines: {read}"
    );
    assert!(
        read.contains("<untrusted_input_"),
        "file text reaches the script marked as untrusted: {read}"
    );
    assert_eq!(
        (write, replace, delete),
        ("Wrote 1 byte.", "Made 1 replacement.", "Deleted the file."),
        "each tool that changes files says what it did, as trusted text"
    );

    let check = vfs
        .acquire(Origin::new("check"))
        .expect("the filesystem acquires");
    assert_eq!(
        check.read_string("/out/new.md").expect("the file is read"),
        "x",
        "Write made the file and the directory on the way to it"
    );
    assert_eq!(
        check.read_string("/notes.md").expect("the file is read"),
        "hello\nthere\n",
        "StrReplace changed the one text"
    );
    assert!(
        !check.exists("/old.md").expect("the filesystem answers"),
        "Delete removed the file"
    );
}

#[tokio::test]
async fn without_a_plugin_directory_a_prompt_requiring_a_shipped_pack_is_refused() {
    for pack in ["std", "bash", "web"] {
        let host = server_host(serving(requiring(pack)), &Registry::new(), None, None);
        let [mcp] = host.plugin_reports() else {
            panic!(
                "the linked Plugin is the one step, so no directory is read: {:?}",
                host.plugin_reports()
            );
        };
        assert!(mcp.plugins().is_empty(), "{:?}", mcp.plugins());

        let result = run_prompt(host).await;
        let Err(message) = result else {
            panic!("the Host refuses the prompt as it parses: {result:?}");
        };
        assert!(
            message.contains(&format!(
                "- {pack} is unavailable: runvil/{pack} is not installed"
            )),
            "the refusal names the Pack no Plugin inserted: {message}"
        );
    }
}

/// A prompt that declares `bash` and returns what `bash/shell` answers
/// for a command that reads `/notes.md`, writes a file under a directory
/// it makes, and edits `/notes.md` in place.
const RUNS_A_COMMAND: &str = "---\nname: shells\ndescription: runs a command\nrunvil: 0\n\
    uses:\n  - bash\n---\n\n## Only\n\n```lua\n\
    return tools.call('bash/shell', {\n\
      command = 'cat /notes.md; mkdir /out; echo x > /out/new.md; sed -i s/world/there/ /notes.md',\n\
    })\n```\n";

#[tokio::test]
async fn a_prompt_runs_a_command_over_its_runs_files_through_the_bashkit_library_plugin() {
    let (dir, _library) = plugin_dir_of("runs-a-command", BASHKIT_LIBRARY);
    let host = server_host(
        serving(RUNS_A_COMMAND),
        &Registry::new(),
        None,
        Some(dir.path()),
    );
    assert_no_failures(&host);
    assert_eq!(loaded(&host), ["bashkit"]);

    let vfs = VfsRef::default();
    vfs.acquire(Origin::new("seed"))
        .expect("a fresh memory filesystem acquires")
        .write("/notes.md", b"hello\nworld\n")
        .expect("the file is seeded");

    let result = run_prompt_over(host, vfs.clone()).await;
    let Ok(Ok(text)) = result else {
        panic!("the run ends with its text: {result:?}");
    };
    assert!(
        text.contains("<untrusted_input_"),
        "a command's output reaches the script marked as untrusted: {text}"
    );
    assert!(
        text.contains("Exit code: 0\n\nCommand output:\n\n```\nhello\nworld\n```"),
        "the shell reads the file through the run's filesystem, and each change succeeds, \
         so the command prints the file alone: {text}"
    );
    let check = vfs
        .acquire(Origin::new("check"))
        .expect("the filesystem acquires");
    assert_eq!(
        check.read_string("/out/new.md").expect("the file is read"),
        "x\n",
        "the shell made the directory and the file in the run's filesystem"
    );
    assert_eq!(
        check.read_string("/notes.md").expect("the file is read"),
        "hello\nthere\n",
        "the shell edited the seeded file in place"
    );
}

#[test]
fn no_setting_reads_no_file_and_gives_no_servers() {
    assert_eq!(read_mcp_servers(None), json!({}));
}

#[test]
fn a_missing_mcp_file_gives_no_servers() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    assert_eq!(
        read_mcp_servers(Some(&dir.path().join("absent.json"))),
        json!({})
    );
}

#[test]
fn an_unreadable_mcp_file_gives_no_servers() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    // A directory in the file's place fails the read for a reason other
    // than a missing file.
    let path = dir.path().join("mcp.json");
    std::fs::create_dir(&path).expect("directory in the file's place");
    assert_eq!(read_mcp_servers(Some(&path)), json!({}));
}

#[test]
fn a_malformed_mcp_file_gives_no_servers() {
    for text in ["{ \"mcpServers\": ", "[]", "{ \"mcpServers\": [] }"] {
        let (_dir, path) = mcp_file(text);
        assert_eq!(
            read_mcp_servers(Some(&path)),
            json!({}),
            "`{text}` is not an object with an mcpServers object"
        );
    }
}

#[test]
fn a_file_without_mcp_servers_gives_no_servers() {
    let (_dir, path) = mcp_file(r#"{ "inputs": [] }"#);
    assert_eq!(read_mcp_servers(Some(&path)), json!({}));
}

#[test]
fn an_entry_that_is_not_an_object_is_handed_on_for_the_pack_to_refuse() {
    let (_dir, path) = mcp_file(r#"{ "mcpServers": { "a": 1, "b": { "url": "x" } } }"#);
    assert_eq!(
        read_mcp_servers(Some(&path)),
        json!({ "a": 1, "b": { "url": "x" } }),
        "one bad entry does not drop the file's other servers"
    );
}

#[test]
fn a_valid_mcp_file_reads_back_its_servers_object_and_ignores_other_keys() {
    let (_dir, path) = mcp_file(
        r#"{
            "inputs": [{ "id": "token" }],
            "mcpServers": {
                "wg21": { "url": "https://example.test/mcp" },
                "GitHub": { "command": "x", "args": ["a"] },
                "docs": { "url": "https://example.test/docs" }
            }
        }"#,
    );
    assert_eq!(
        read_mcp_servers(Some(&path)),
        json!({
            "wg21": { "url": "https://example.test/mcp" },
            "GitHub": { "command": "x", "args": ["a"] },
            "docs": { "url": "https://example.test/docs" }
        }),
        "the servers object is handed on verbatim, names and entries alike"
    );
}
