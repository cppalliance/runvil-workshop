---
name: Check what Workshop hands its Packs, and run Workshop against a real Gateway
overview: Workshop's server is tested with a stand-in for the Gateway and with no window. That leaves two kinds of gap, which this plan closes with twelve tests. Two run in a normal test run and check joins in the code that builds Workshop's Host, each of which was broken with every test still passing. Ten run Workshop against a real Gateway and a real model, through its server, its program, and its window, and are ignored unless asked for. It changes no production code.
todos:
  - id: plan-copy
    content: "Commit this plan alone on the branch qa-plan-tests"
    status: completed
  - id: mcp-servers
    content: "Add the test of the configured MCP servers to crates/workshop/server/src/agents/tests.rs"
    status: completed
  - id: web-search
    content: "Add the test of the Gateway a web search is lent to crates/workshop/server/src/agents/tests-gateway.rs"
    status: completed
  - id: socket-tests
    content: "Add crates/workshop/server/tests/it/live.rs with the six ignored tests, and name the module in tests/it/main.rs"
    status: completed
  - id: kill-test
    content: "Add crates/workshop/server/tests/it/live_stop.rs with the one ignored test, and name it in tests/it/main.rs for Unix only"
    status: completed
  - id: program-test
    content: "Add crates/workshop/server/tests/it/live_program.rs with the one ignored test, and name it in tests/it/main.rs for Linux only"
    status: completed
  - id: window-tests
    content: "Add crates/workshop/server/tests/it/live_window.rs with the two ignored tests, and name it in tests/it/main.rs for Linux only"
    status: completed
  - id: breaks
    content: "Apply each of the three breaks, confirm the test named for it now fails, and revert it"
    status: completed
  - id: runs
    content: "Run the ten ignored tests against a real Gateway with a small local model, several times each, and see no process left"
    status: completed
  - id: verify
    content: "Run the verification commands"
    status: completed
  - id: pr
    content: "Push the branch qa-plan-tests to the fork and open one PR against cppalliance/runvil-workshop"
    status: in_progress
---

# Check what Workshop hands its Packs, and run Workshop against a real Gateway

<product-contract>

## Product Requirements

Workshop's server is tested with a stand-in for the Gateway, which echoes or replays text, and with no window. That leaves two kinds of gap. Two joins in the code that builds Workshop's Host can be broken while every test passes, which was shown by breaking them. And nothing runs Workshop against a real Gateway and a real model, through its server, its program, or its window. The code is correct today, so the first kind is missing protection, not a bug.

- Problem and users:
  - The MCP servers. Workshop's agents get tools from MCP servers, which come from the `mcp.json` file that the `[agents] mcp` setting names. `server_host` in `crates/workshop/server/src/agents.rs` reads that file with `read_mcp_servers` and hands the result to `with_plugins`, which installs the mcp Pack over it. Tests cover both halves and not the join. With `server_host` changed to hand `with_plugins` an empty object, a Workshop with configured MCP servers runs with none of them, and every test passes.
  - The Gateway a web search is lent. The web Pack sends each search to a Gateway, and gets that Gateway from Workshop. `resources`, in the same file, offers every library Plugin a function, and the Harness calls it as each tool call starts. The function is `reachable_gateway`, which reads the address and the key of the Gateway that Workshop's registry holds at that moment. The comment on `resources` promises the result: "a replaced Gateway serves the next call". Tests check the reader, and one shows that a model's search reaches a Gateway. None checks that the read happens at each call. With `resources` changed to read the Gateway once, when the Host is built, a Gateway that replaces the first gets no search, and every test passes. No test has a prompt's own script call a web tool through the web library Workshop ships and read what the call returns, either.
  - A real Gateway behind the server. The server's tests start the real router in the test's process and point it at a small stand-in server. That checks Workshop's own code well, and it cannot show a fault that needs the real Gateway or a real model's replies: a key the Gateway refuses, a requirement the model does not meet, a tool call the model must choose to make. No test in any Runvil repository launches a Workshop agent whose rounds reach a real Gateway that serves a real model.
  - A Gateway that dies. A person can lose the Gateway while an agent is replying: it crashes, or the machine kills it. The run must end, the client must be told, and Workshop must go on serving. `gate_model_failure_surfaces_an_error_and_the_next_input_works` covers a stand-in that answers status 500 before any text. Nothing cuts a streaming reply off part way.
  - The program. Workshop attaches to a Gateway that is already running, and otherwise starts the installed one. It finds a running Gateway through the discovery file the Gateway writes under the person's home folder. `a_live_file_attaches_without_looking_for_a_sibling_exe` checks the plan Workshop makes from such a file, with a stand-in. The installer checks start the installed program and compare addresses, in another repository and only for an installed copy. Nothing in this repository starts the program built from its own source beside a real Gateway.
  - The window. A person uses Workshop through its window: they type in the chat box, read the chat panel, and close the window when done. The interface's tests run its parts in a simulated page, and the server's tests have no window. No test types into the real window and reads the reply it shows, and no test closes the window and checks the Gateway that Workshop started.
  - The users are the people who run agents in Workshop, who configure MCP servers for them or search the web from them, and the engineers who change Workshop, the Harness, or the Gateway.
- Goals:
  - Each of the three breaks listed in the Functional Specification makes at least one test fail.
  - A prompt's script calls `web/search` through the shipped web library and reads the Gateway's results.
  - Six socket tests each make a real run through Workshop's server and a real Gateway: the shipped hello prompt, a chat that recalls an earlier message, two model roles, a tool the model calls, a larger prompt, and a Gateway that refuses a caller with no key.
  - A kill test starts a Gateway, launches an agent that asks for a long reply, kills the Gateway once the reply has started, and checks three things: the client is told the round failed, the run is recorded as ended with an error, and Workshop still answers a new connection and launch.
  - A program test starts the Workshop program beside a running Gateway and checks that it reports that Gateway's address and that a chat with the built-in agent gets a reply.
  - Two window tests drive the real window. The typing test types a message in the chat box and finds the typed text and a reply in the chat panel. The closing test has Workshop start a Gateway, closes the window with the title bar's own Close button, and gets a chat reply from that Gateway afterwards.
  - No window opens on the screen of the person who runs the tests, and the program cannot reach a Workshop that person has open.
  - A normal test run skips the ten live tests. A person asks for them by name and names the Gateway in the environment.
- Non-goals:
  - Any change to production code.
  - A call to an MCP server's tool. The mcp Pack's own tests, in the Harness repository, check calls against a stand-in server.
  - The web Pack's fetch tool, and the form of a search's answer in every case. The web Plugin's own repository tests both.
  - A run of the live tests in CI. No workflow has a Gateway with a chat model.
  - Fetching a model, or starting a Gateway where the test need not own it. The kill test starts the Gateway it kills, and in the closing test Workshop starts its own. Every other live test is handed a Gateway that is running.
  - A Gateway that is asked to stop. Its `POST /shutdown` lets a reply in flight finish, so the run sees no failure.
  - A check of what the model says, beyond what any sensible reply satisfies.
  - The program and window tests on systems other than Linux, and the kill test on systems other than Unix. The tools that give the program a display of its own are Linux tools, Tauri's WebDriver server has no macOS support, and the kill test kills a process group.
  - The Quit command, which is a different path from closing the window.
- Success criteria:
  - With each break applied, `cargo nextest run -p workshop-server` fails, and it passes again once the break is reverted.
  - A normal run of the `workshop-server` crate has exactly two more tests than before, and skips ten more.
  - With a Gateway running that serves a chat model of at least 32,768 tokens of context, with `trust_loopback` off, the six socket tests pass when run one at a time.
  - The kill test, the program test, and each window test pass in three runs in a row.
  - After each of those runs, no process the test started is left running, a Gateway that the test or Workshop started included.
  - The verification commands in the Testing Plan pass.
- Constraints:
  - Every existing test in `crates/workshop/server/src/agents/tests.rs` and `crates/workshop/server/src/agents/tests-gateway.rs` keeps every assertion it makes today.
  - The two tests of a normal run start no MCP server and reach no network beyond the loopback address.
  - The tests add no dependency. The live tests use the suite's existing support, and the window tests speak the WebDriver protocol with the HTTP client the suite already uses.
  - The kill test refuses to start when a Gateway already answers at the address, so that it never kills one it did not start.
  - The person who runs the live tests must have these tools: `pkill` for the kill test; `setsid`, `dbus-run-session`, `xvfb-run`, `pgrep`, `pkill`, and `lsof` for the program and window tests; and `tauri-driver` and WebKit's `WebKitWebDriver` for the window tests.
- Open questions: None

## Functional Specification

The two tests of a normal run build Workshop's Host through `server_host` and run a prompt on it. Each live test reads its settings from the environment, and then reads what a client of Workshop would read: the frames of the agents socket, the program's local server, or the page in the window. Nothing a user of Workshop sees changes.

- Actors and workflows:
  - A developer or CI runs the test suite, which runs the two tests and skips the ten.
  - The socket tests: a person starts a Gateway that serves a chat model, sets two variables, and runs `cargo nextest run -p workshop-server -E 'test(live::)' --run-ignored ignored-only -j 1`.
  - The kill test: a person makes sure no Gateway is running at the address, sets four variables, and runs `cargo nextest run -p workshop-server -E 'test(live_stop::)' --run-ignored ignored-only`.
  - The program test: a person starts a Gateway, sets three variables, and runs `cargo nextest run -p workshop-server -E 'test(live_program::)' --run-ignored ignored-only`.
  - The window tests: a person sets four variables and asks for one test by name. The typing test needs a Gateway already running. The closing test needs none running.
- Inputs and outputs:
  - The variables. `RUNVIL_E2E_GATEWAY` is the Gateway's API root, such as `http://127.0.0.1:8189/v1`, and `RUNVIL_E2E_KEY` is its key. `RUNVIL_E2E_GATEWAY_HOME` is the home folder the Gateway reads its settings from. `RUNVIL_E2E_GATEWAY_BIN` is the Gateway program, and `RUNVIL_E2E_WORKSHOP_BIN` is the Workshop program. The socket tests read the first two. The kill test reads those and the Gateway's home folder and program. The program test reads the API root, the home folder, and the Workshop program. The window tests read all but the Gateway program.
  - The MCP servers. One test writes an `mcp.json` that names two servers, builds the Host from it, and runs a prompt that asks the mcp Pack which servers it has.
    - The file names the servers `docs` and `search`, each at `http://127.0.0.1:1/mcp`, where nothing listens.
    - The prompt requires the mcp Pack, calls `mcp/get_dynamic_tools` with no arguments, and returns the namespaces of the listing, joined by commas.
    - The run returns `docs,search`.
  - The web search. One test builds the Host under one Gateway, replaces that Gateway with a second, and runs a prompt that searches.
    - Two stand-in Gateways each answer `POST /v1/tools/web_search` under their own key with the query they were asked and one result. The first titles its result `First` and the second `Second`. Each refuses a search under any other key.
    - The registry holds the first Gateway. `server_host` builds the Host over a Plugin directory that holds the web library. Then the second Gateway replaces the first.
    - The prompt requires the web Pack and returns what `tools.call('web/search', { query = 'docs' })` answers.
    - The answer holds the second Gateway's reply as compact JSON, with the query `docs` and the result titled `Second`, inside the marking for untrusted text.
  - The socket tests. Each writes its agents into a new agents folder, binds Workshop's router to the Gateway the environment names, launches one agent over the agents socket, and reads the frames the client would read.
    - Hello: the text of the Harness repository's `prompts/hello.md` runs as an agent. The client reads a reply that is not empty, then the run's end.
    - Chat: the built-in chat agent is told a word and then asked for it. The second reply holds the word.
    - Roles: a prompt drafts under `creative` and critiques under `strong`. The client reads two replies, neither empty.
    - Tool: a prompt offers the model one tool and asks it to call that tool. The client reads a tool call and a reply after it.
    - Larger prompt: a prompt with an argument that has a default, the store, a tool its Lua calls, and a fanout of two tasks. The client reads three replies, none empty.
    - Key: a chat request with no key is answered 401, and the hello agent still ends with text, because Workshop sends its key.
  - The kill test owns its Gateway from start to kill.
    - It starts the Gateway program with `--no-tray --profile default` and the home folder, whose `.runvil/gateway.toml` binds the address and serves a chat model in its `default` profile. The program runs in a process group of its own, and the test waits until a chat request succeeds.
    - It launches an agent that asks for the numbers from 1 to 300, waits for the first piece of the reply, and kills every process of the Gateway at once: the program it started, which only supervises, the process that serves, and the model's process.
    - The client reads an error frame whose message is "Model turn failed in agent `Count`".
    - The run log records run 1 as ended with an error.
    - A new connection lists the agents, a launch is acknowledged, and the client reads an error frame that says the model catalog could not be fetched.
  - The program test owns the Workshop program from start to kill. The Gateway is the person's to start.
    - It starts the program in a new folder, with the Gateway's home folder, in a session of its own, on a display that `xvfb-run` makes and a message bus that `dbus-run-session` makes.
    - The folder holds a `workshop.toml` that names the Plugin folder and, because the file must name a Gateway, an address nothing listens on.
    - The test finds the loopback port on which the program serves Workshop's page.
    - `GET /gateway/origin` on that port answers the running Gateway's address.
    - Over the agents socket, the built-in chat agent is launched, sent one message, and replies with text that is not empty.
  - The window tests. Each starts `tauri-driver` in a session of its own, on a display and a message bus of its own, and has it open the Workshop program's window.
    - Typing: once the model picker names the Gateway's model and the chat box's button offers to send, the test types "Reply with one short greeting." into the chat box and presses Enter. The chat panel shows that text as the person's message, and a reply that is not empty.
    - Closing: no Gateway answers at the address when the test starts. Workshop starts the Gateway program that sits beside it. Once that Gateway answers a chat request, the test clicks the Close button of the title bar Workshop draws. Workshop stops serving its page, and the Gateway still answers a chat request.
- States and validation:
  - A missing variable fails a live test with a message that names it.
  - Neither MCP server ever becomes ready. The listing names each server whatever its status, and the test reads only the names.
  - The socket tests bind the router without its serving loop, so each fetches the Gateway's model list itself and publishes it to Workshop's catalog.
  - A Gateway that already answers at the address fails the kill test and the closing test before anything is started. No Gateway at the address fails the program test and the typing test before the program is started.
  - For the closing test, the home folder's `.runvil` holds a `gateway.toml` that binds the address and a `gateway.state.toml` that selects a profile with a chat model, because Workshop starts the Gateway with no options.
- Errors and recovery:
  - An error frame before the awaited event fails a socket test and prints the frame.
  - Every process a live test started is killed when the test ends, whether it passed or failed: the Gateway's process group in the kill test, and the whole session in the program and window tests. The program test waits until they are gone.
- Security and privacy behavior:
  - The key comes from the environment. No key is written into the repository.
  - The program is started without `WAYLAND_DISPLAY` and with `GDK_BACKEND=x11`, so its window can be drawn only on the display made for it.
  - Its message bus is its own, so its single-instance check cannot find, or hand over to, a Workshop the person has open.
- Acceptance criteria:
  - Break 1: in `server_host`, hand `with_plugins` the value `serde_json::json!({})` in place of `read_mcp_servers(mcp)`. The MCP test fails, and it is the only test of the `workshop-server`, `workshop-agents`, and `workshop-run-log` crates that fails because of the break.
  - Break 2: in `resources`, read `reachable_gateway(&registry)` once into a value and hand `provide_gateway_resources` a function that returns a copy of it. The web search test fails, and it is the only test that fails because of the break.
  - Break 3: in `resources`, do not call `provide_gateway_resources`. The web search test fails.
  - The six socket tests pass in three runs in a row, one at a time, each run from a fresh start of the Gateway.
  - The checks of the kill test, the program test, and each window test hold in three runs in a row, and no process is left after any of them.

</product-contract>
<implementation-contract>

## Technical Design

The change is test code: one test in each of two existing files of the server's unit tests, and four new modules of its integration suite. No module of the product, interface, or stored format changes.

- Architecture:
  - Each test of a normal run runs a prompt on the Host `server_host` builds, as the other tests of its file do.
  - The live tests are in the server's existing integration suite and reuse its socket client and router binding. The socket tests and the kill test bind Workshop's router in the test's process. The program and window tests start the Workshop program.
  - In the window tests, `tauri-driver` starts the Workshop program and exposes its page over WebDriver, which the tests reach over HTTP.
  - Each kind of live test has a module of its own, so that asking for `live::` does not select a test that needs a different setup.
- Modules and interfaces: None
- File and public API changes:
  - `crates/workshop/server/src/agents/tests.rs` gains one test. It uses helpers the file already has: `mcp_file`, `serving`, and `run_prompt`.
  - `crates/workshop/server/src/agents/tests-gateway.rs` gains one test, a stand-in Gateway that answers searches, the prompt, and a helper that checks a request's key, which the file's existing stand-in now uses too. It uses `plugin_dir_of`, `WEB_LIBRARY`, `serving`, `assert_no_failures`, and `run_prompt` of its parent module.
  - `crates/workshop/server/tests/it/live.rs` is new: four prompts as text, seven helpers, and six tests. `STEP`, `gateway`, `live_server`, and `launched` are visible to the rest of the test crate, for the other live modules.
  - `crates/workshop/server/tests/it/live_stop.rs` is new: two prompts as text, a guard that kills the started Gateway's process group when dropped, a helper that starts the Gateway in that group and waits for it, and one test.
  - `crates/workshop/server/tests/it/live_program.rs` is new: a guard that owns a session, a helper that starts a command in it, a helper that writes the settings folder, a helper that finds the program's port, and one test. The first three take any command and are visible to the rest of the test crate, for the window tests.
  - `crates/workshop/server/tests/it/live_window.rs` is new: a small WebDriver client over the suite's HTTP client, helpers that ask the Gateway for a reply, and two tests.
  - Each live test is marked `#[ignore]` with the reason.
  - `crates/workshop/server/tests/it/main.rs` names the four modules: `live` on every system, `live_stop` under `#[cfg(unix)]`, and `live_program` and `live_window` under `#[cfg(target_os = "linux")]`. So the suite still builds on Windows, where CI runs it.
  - `plan/2026-11-20-1-pack-hand-offs-and-real-gateway.plan.md` is this plan.
- Data, persistence, failure, security, and privacy constraints: None

</implementation-contract>
<verification-contract>

## Testing Plan

The two tests of a normal run are checked by their breaks: each break must fail the test named for it. The ten live tests are checked by running them against a real Gateway, several times each, and by looking for processes left behind.

- Unit:
  - `cargo nextest run --locked -p workshop-server -E 'test(mcp)'` runs the nine tests that name the mcp Pack: eight existing and one new.
  - `cargo nextest run --locked -p workshop-server -E 'test(/agents::tests::gateway::/)'` runs the five tests of `tests-gateway.rs`: four existing and one new.
- Integration and end-to-end:
  - Run the six socket tests one at a time against a Gateway that serves a small local chat model, from a fresh start of the Gateway, three runs in a row.
  - Run the kill test three times in a row with a Gateway built from source and a small local chat model.
  - Run the program test three times in a row with the Workshop program built from this repository and a running Gateway.
  - Run the typing test three times in a row beside a running Gateway.
  - Run the closing test three times in a row with no Gateway running and the Gateway program beside the Workshop program.
- Regression, security, and performance:
  - Apply each break, run the server's tests, see the named test fail, and revert the break.
  - After each run of the kill test, the program test, and the window tests, list the processes and see none the test started, and see the Gateway's port free.
  - Run the server's tests without asking for ignored tests, and see the ten skipped.
- Exit criteria:
  - `cargo nextest run --locked -p workshop -p workshop-server -p workshop-server-api`
  - `cargo clippy -p workshop -p workshop-server -p workshop-server-api --all-targets`, with `CARGO_BUILD_WARNINGS=deny`
  - `cargo fmt --all --check`
  - `cargo test -p build-xtask`
  - `node --test crates/workshop/ui/test/docs-claims.mjs`

</verification-contract>
<decision-record>

## Decision Record

- Decisions:
  - Each gap of the two tests of a normal run was shown before its test was written, by a break under which every test passed but one that also fails without the break (see the notes). The user's rule, given on 2026-11-18: "If existing tests already cover these behaviors, we shouldn't be raising a PR that introduces duplicate coverage."
  - The MCP test reads the Pack's listing of its servers, not a tool call. The listing names a server that is not ready, so the test needs no stand-in server, and it still fails when the servers do not arrive.
  - The MCP servers point at port 1 on the loopback address, where a connection is refused at once.
  - In the web search test, the Gateway is replaced between the building of the Host and the search. That is the order in which break 2 goes wrong, and it is the promise the comment on `resources` makes.
  - That test makes one search, after the replacement. Break 2 sends that search to the first Gateway, so one search is enough to fail under it.
  - Each stand-in Gateway refuses any key but its own. So the answer also shows that the key is read together with the address.
  - The search is called from the prompt's own script. That is the one kind of call the web library had no test for in this repository, and it makes the answer readable as the run's text.
  - The web search test sits in `tests-gateway.rs`, whose subject is the Gateway that a tool call of a library Plugin is lent.
  - The live tests are ignored by default and read the Gateway's address from the environment. The user's direction, given on 2026-11-20: "All these tests should be runable locally. We aren't dealing with running tests in CI yet."
  - The socket tests bind Workshop's router in the test's process. That is the real server code against the real Gateway, with no window and no packaged program.
  - Each socket test waits up to 120 seconds for the event it wants. The suite's own 10-second limit is tuned for a stand-in that answers at once.
  - The socket tests are run one at a time. They share one Gateway, and a small local model serves one request at a time.
  - The hello prompt's text is repeated in the socket tests. Workshop's repository does not hold the Harness repository's `prompts/` folder.
  - The kill test starts and kills its own Gateway. A test that is handed a running Gateway can only ask it to stop, and a Gateway asked to stop finishes the reply first.
  - Every process of that Gateway is killed with the operating system's kill, which gives it no chance to finish. That is what a crash looks like to Workshop.
  - The kill goes to a process group. The program the test starts only supervises: it starts the process that serves, and that process starts the model's process. None of them leaves the group of the first, so the test starts the program in a group of its own and kills the group with `pkill -KILL -g`.
  - "Workshop keeps running" is checked as: it lists its agents, acknowledges a launch, and says why the run cannot start. With no Gateway, no run can start, even of a prompt that asks no model, because a run's first step asks the Gateway for its models.
  - The killed run's end is read from the run log, where the result is one error. The client's frame says only that the round failed.
  - The Workshop program gets a display and a message bus of its own. It has no mode without a window: it takes `--version` and `--plugins`, and any other start opens the window. The user chose a virtual display for these tests on 2026-11-20.
  - The program's settings file names an address nothing listens on. Workshop reads the discovery file first and its settings second, so the address it reports can only have come from the running Gateway.
  - The program's port is found by listing the loopback ports of its processes and asking each for Workshop's page, as the installer checks do. The program always binds a port the system picks.
  - The program runs in a session of its own and the test kills the session. Its message bus starts helper processes that a kill of the program alone would leave behind.
  - The window is driven through `tauri-driver`. It reads the page inside the real window, and Tauri's guide says it needs no change to the application. The user chose it on 2026-11-20.
  - The window tests speak WebDriver over plain HTTP, with no client library. The five commands they need are small, and the suite gains no dependency.
  - The window is closed with the Close button of the title bar Workshop draws, which is what a person clicks.
  - The closing test has Workshop start the Gateway. A Gateway that was running before Workshop would be expected to outlive it. The case worth a test is the Gateway that is Workshop's own.
  - The typing test waits until the model picker names the Gateway's model before it types, so it never types into a window that has not found its Gateway.
  - It also waits until the chat box's button offers to send, which it reads from the button's `data-action`. Workshop ignores Enter until the agent's run has started, so a message sent before that stays in the chat box and no reply comes.
  - One plan and one branch, `qa-plan-tests`, hold all twelve tests. The user's direction, given on 2026-11-20: "All of the work for this effort should be in one branch."
- Rejected alternatives:
  - A stand-in MCP server and a call of its tool from a Workshop prompt. The mcp Pack's own tests, in the Harness repository, call a tool of a stand-in server from a model round and from a script, in `crates/plugin-mcp/tests/it/calls.rs`. Workshop's own part is to hand the Pack its servers, which the MCP test checks. No break of Workshop's code was found that such a test would catch and those do not.
  - A search before the replacement and one after it. The one search after the replacement already fails under break 2, so an earlier search would add a second run and no check.
  - One prompt that calls a tool of each of the four Packs. The std and bashkit libraries each have a test of a prompt's call already, so their two calls would be checked twice.
  - Socket tests that start their own Gateway. They would tie this repository to the Gateway's settings format. Only the kill test starts one, because it must own the Gateway it kills, and it writes no settings.
  - `POST /shutdown` on a Gateway the kill test is given. Measured: a streamed reply of 114 pieces went on to its end, in 2.3 seconds, after the request was answered 202.
  - Finding the process of a running Gateway from its port and killing it. It could kill a Gateway the person relies on.
  - Killing only the program the kill test started, which is what the test first did. The serving process then sees that its supervisor is gone and shuts down in good order, in about 0.6 seconds, and a reply shorter than that finishes first. Then the run ends with text and no error comes. See the notes.
  - Starting the installed Workshop program. It may be an older version than the code under test.
  - Loading Workshop's page in a headless browser. It would test the interface outside the window, and it cannot close the window.
  - Keyboard and mouse events from `xdotool`. It cannot read the chat panel.
- Assumptions, risks, and notes:
  - `a_valid_mcp_file_reads_back_its_servers_object_and_ignores_other_keys` checks the reader of `mcp.json`, and `the_servers_value_is_the_mcp_packs_server_list` checks that the Pack receives the value `with_plugins` is handed. Both pass under break 1.
  - `reachable_gateway` has four tests of its own, and `a_message_sent_while_a_tool_call_runs_is_a_user_message_the_next_request_holds` shows that a model's search reaches the Gateway that was registered before Workshop built its Host. All pass under break 2.
  - The web search's answer is compared with the exact text the web Pack hands on. The pinned web Plugin's own test `a_search_posts_its_arguments_to_the_gateway_the_resources_name_and_answers_its_results` fixes that form. A new pin that changes it fails the web search test, and the person who moves the pin updates it.
  - The two tests of a normal run serve the test "Run a prompt whose Lua calls one tool from each of the four Packs. Each call returns a result." of the QA plan of attack, the list of tests this work automates. With them, a prompt's call of each Pack's tool is checked: the file tools by `a_prompt_reads_and_changes_files_of_its_run_through_the_std_library_plugin`, the shell by `a_prompt_runs_a_command_over_its_runs_files_through_the_bashkit_library_plugin`, the web tools by the web search test, and the MCP tools by the Harness's tests named under Rejected alternatives.
  - The live tests serve ten tests of that list. Six are the socket tests: the hello prompt through Workshop and a Gateway, the chat agent recalling a word, the two roles, the model's tool call, the larger prompt, and loopback trust turned off. The other four read: "Stop the Gateway mid-run. The run ends with one `Error`, and Workshop keeps running.", "Start Workshop beside a running Gateway. It reports that address, and a run ends with text.", "Type a message in the real Workshop window. The reply appears in the chat panel.", and "Close Workshop's window and send the Gateway a chat request. It still answers."
  - The window tests find the chat box, its button, the messages, and the Close button by the class names the interface gives them. A rename there fails the tests, and the person who renames updates them.
  - The program's state goes in the test's folder, because Workshop keeps its state beside the settings file it loaded.
  - Live tests started before the Gateway has finished loading its model fail at once. The Gateway's log line `command finished` for `load-profile` marks the end of loading.
  - `stalled_browser_cleanup_is_bounded_after_gateway_disconnect`, in `crates/workshop/server/tests/it/realtime_relay/overload.rs`, fails on the author's Linux machine on unchanged code, at `the Gateway fills the relay's browser send`, after about ten seconds. It fails the same way on unchanged `upstream/master` at commit `73e4fccd`. CI runs the server's tests on Windows only. This plan does not touch it.
  - The break results and the exit criteria below were measured on `upstream/master` at commit `73e4fccd`, onto which the branch was rebased on 2026-11-21. Workshop builds its Host through the Harness's `Host::builder` there, and the two tests of a normal run were changed to build theirs the same way.
  - Result of break 1: the 308 tests of the `workshop-server`, `workshop-agents`, and `workshop-run-log` crates ran. The MCP test failed, and the only other failure was `stalled_browser_cleanup_is_bounded_after_gateway_disconnect`.
  - Result of break 2: the 258 tests of `workshop-server` ran. The web search test failed, with the first Gateway's result in the answer. The only other failure was `stalled_browser_cleanup_is_bounded_after_gateway_disconnect`.
  - Result of break 3: the web search test failed with "web_search: the host offers no web search right now". `a_message_sent_while_a_tool_call_runs_is_a_user_message_the_next_request_holds` failed too, and so did `stalled_browser_cleanup_is_bounded_after_gateway_disconnect`.
  - The live runs used a Gateway built from `cppalliance/runvil-gateway` at commit `52fd92d`, serving Qwen3 0.6B from a local file. The closing test used the Gateway release Workshop is pinned to, `gateway-v0.3.1`, beside the Workshop program.
  - Found while writing the socket tests: the built-in chat agent requires a context of at least 32,768 tokens for its `default` role. Against a model declared with 8,192, the launch ends with the requirements notice, which Workshop relays whole in an error frame.
  - Found while writing the socket tests: a real Gateway answers a wrong key with status 401 even from a loopback caller it would trust with no key. Workshop always sends its key, so the key in its settings must be right.
  - Unexplained: twice, with the model loaded and several of the socket tests running side by side straight after the Gateway started, one model round failed and Workshop reported "Model turn failed". Workshop's message does not carry the cause. It was not seen with the tests run one at a time, nor in direct requests to the Gateway, nor in runs through the Harness alone.
  - Result of the first runs of the socket tests: from a fresh start of the Gateway, with the tests run one at a time, all six passed in three runs in a row, in 14, 5, and 6 seconds. Run side by side on a Gateway that had been up for a while, all six passed in four runs in a row.
  - Observed in every run of the kill test: the recorded error has the code `Transport` and its message ends "unexpected EOF during chunk size line".
  - Found in the kill test: its first form killed only the program it started. It passed three runs in a row, and then failed 3 runs of 38 when every test was run again. In each failed run the client got no error frame within the deadline. The Gateway's log showed why: "the supervisor closed stdin; shutting down", and its exit 0.6 seconds later. The model had cut its list short ("1" to "30", then "...", then "300"), so the reply of 129 pieces finished inside that time, and the run rightly ended with text. Workshop was not at fault. The test had not killed the process that serves.
  - Result of the runs of the kill test with the whole group killed: it passed 50 runs of 50, in 4.3 to 5.5 seconds a run. In none of them did the Gateway's log show a shutdown in good order, and no Gateway or model process was left after any run.
  - Result of the runs of the program test: it passed in four runs, the first in 12 seconds and the next three in about 2 seconds each. No process the test started was left after any run.
  - Found while writing the closing test, with Workshop at commit `68c7249`, and not tried again since: Workshop can start the Gateway release it is pinned to, and cannot start a Gateway built from that repository's `master` at commit `52fd92d`. Workshop holds the launch lock while it starts the Gateway and waits for the discovery file. The newer Gateway tries to take the same lock, waits ten seconds, and exits with "settle which process launches the Gateway: no launch winner became attachable within 10s". Workshop then reports "the launched gateway exited (exit status: 1) without writing a gateway discovery file".
  - Result of the first runs of the window tests: each passed in three runs in a row, about 3 seconds a run, and no process or open port was left after any of them.
  - Found in the typing test: it first waited only for the model picker, and failed 3 runs of 18 when every test was run again. In each failed run the typed text was still in the chat box and the chat panel was empty, because the test had pressed Enter before the agent's run started. Workshop was not at fault. With the second wait, the test passed 30 runs of 30, about 3.3 seconds a run. The closing test passed 5 runs of 5.
  - Those live runs were made on 2026-11-20, the last of them on `upstream/master` at commit `4666b17f`. Result of the live runs made again on 2026-11-21, after the rebase onto `73e4fccd`: the six socket tests passed, one at a time, in 6 seconds. The program test passed 3 runs of 3, the typing test 5 of 5, the kill test 5 of 5, and the closing test 3 of 3. No process the tests started and no open port was left at the end.
  - Result of the exit criteria. The three crates ran 343 tests: 342 passed, 14 were skipped, and `stalled_browser_cleanup_is_bounded_after_gateway_disconnect` failed. `cargo clippy -p workshop -p workshop-server -p workshop-server-api --all-targets`, `cargo fmt --all --check`, `cargo test -p build-xtask`, and the wording check passed. The rest of the workspace, which is not an exit criterion, ran 704 tests, all passed, and 1 was skipped. Clippy over the rest of the workspace, the docs build, and the custom lints were not run locally.
  - CI runs the checks that were not run locally on the draft pull request, cppalliance/runvil-workshop#32. Its ten checks passed on the branch as it stood on 2026-11-20, before the rebase: `check-workshop`, `check-workshop-linux`, `ci-green`, `clippy`, `docs`, `dylint`, `fmt`, `supply-chain`, `test`, and `ui`.
  - `vibelight`, run on each changed test file alone on 2026-11-21, reports no lead in `live_stop.rs`, `live_program.rs`, or `live_window.rs`. In `tests-gateway.rs` it reports one, on the helper `registry_with_gateway`, which this change does not touch. In `tests.rs` it reports six, none on the MCP test. One of them counts the web search test among three tests that repeat a literal, in the wording the file already uses. In `live.rs` it reports nine, all in the new code: two helpers that return a tuple, a `kind` passed as a string to two helpers and always the same to one, a helper with one caller, two tests whose bodies are alike, and two literals repeated across tests. Those nine have not been acted on.
  - This work was first written as six plans, one for each test of a normal run and one for each kind of live test. They were merged into this plan on 2026-11-21, at the user's word: "let's merge those into 1".

### Deferred and Out of Scope

- Deferred: a search that starts while the heartbeat reports the Gateway down. `a_gateway_the_heartbeat_reports_down_is_not_read` checks the reader for that case, and the web search test checks that the reader is asked at each call.
- Deferred: the Quit command, which shuts down a Gateway that Workshop started.
- Out of scope: every other test of the QA plan of attack.

</decision-record>
<project-survey>

## Project Survey

None

</project-survey>
<execution-plan>

## Execution Instructions

- Work items:
  - Commit this plan alone on the branch `qa-plan-tests`, made from `upstream/master`.
  - Add the tests in six commits: the MCP test, the web search test, the socket tests, the kill test, the program test, and the window tests.
  - Apply and revert each break, recording which tests fail.
  - Make the live runs and record their results.
  - Run the exit criteria.
  - Push the branch and open the PR only when the user asks.
- Dependencies and verification:
  - The server's tests need one `npm ci` in `crates/workshop` first.
  - The socket tests, the program test, and the typing test need a running Gateway that serves a chat model.
  - The kill test needs the Gateway program and a home folder whose settings serve a chat model.
  - The program and window tests need the Workshop program built with `cargo build -p workshop`, and the tools named under Constraints. The closing test needs the Gateway release that Workshop is pinned to, beside the Workshop program.
  - No break is ever committed. `git status` shows no change to `agents.rs` before each commit.

</execution-plan>
