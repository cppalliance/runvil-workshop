//! In-process serving: the workshop server on a dedicated thread.
//!
//! [`spawn`] builds the shared state, binds the listener, and serves on its
//! own thread with its own tokio runtime, so an embedding binary (the
//! desktop app) keeps its main thread. The call blocks until the listener
//! is bound - that bind is the readiness signal - and the returned
//! [`ServerHandle`] holds the base URL and a graceful-shutdown switch. The
//! stop side is bounded: a watchdog gives in-flight connections a grace
//! window to drain and then tears the runtime down anyway, and a stopped
//! barrier reports [`Termination`] back through [`ServerHandle::shutdown`],
//! so a held socket can never park the embedding binary.

use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::Duration;

use crate::app::{StateError, config_router, router, state_with_gateway};
use workshop_gateway::{GatewayUpdater, ResolvedGateway};
use workshop_support::Config;

/// How long a signaled shutdown waits for in-flight connections to drain
/// before the watchdog abandons the graceful path. axum's drain waits on
/// every connection it still tracks - a request wedged mid-body never
/// drains - so this bound keeps one stuck client from parking the embedding
/// binary's join forever. (Held WebSockets detach from the drain at upgrade.)
const SHUTDOWN_GRACE: Duration = Duration::from_secs(5);

/// How long either ending waits for the runtime to tear down. Canceled
/// async tasks collapse at their next yield, but a wedged blocking task
/// would otherwise hold the runtime's drop - and with it the embedding
/// binary's join - open indefinitely.
const RUNTIME_TEARDOWN: Duration = Duration::from_secs(1);

/// How a [`ServerHandle::shutdown`] ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Termination {
    /// Every in-flight connection drained within the grace window.
    Graceful,
    /// The grace window lapsed with connections still held open, and the
    /// runtime was torn down anyway.
    Forced,
}

/// A running workshop server on its own thread.
///
/// Dropping the handle without calling [`ServerHandle::shutdown`] still
/// signals the server to stop, but does not wait for it.
#[derive(Debug)]
pub struct ServerHandle {
    url: String,
    config_origin: String,
    gateway: GatewayUpdater,
    initial_gateway_identity: Option<workshop_gateway::discovery::ValidatedConnection>,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
    stopped: mpsc::Receiver<Termination>,
    thread: Option<JoinHandle<std::io::Result<()>>>,
}

impl ServerHandle {
    /// Returns the base URL the server is listening on, for example
    /// `http://127.0.0.1:7910`.
    #[must_use]
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Returns the separate loopback origin serving only the proxied
    /// Gateway config documents (`http://127.0.0.1:<port>`).
    #[must_use]
    pub fn config_origin(&self) -> &str {
        &self.config_origin
    }

    /// Returns the restricted local-Gateway handle used by an embedding
    /// desktop app to replace a sidecar atomically and request shutdown from
    /// the current validated generation.
    #[must_use]
    pub fn gateway_updater(&self) -> GatewayUpdater {
        self.gateway.clone()
    }

    /// Returns the validated local Gateway identity initially published into
    /// server state, or `None` when explicit configuration won resolution.
    #[must_use]
    pub fn initial_gateway_identity(
        &self,
    ) -> Option<&workshop_gateway::discovery::ValidatedConnection> {
        self.initial_gateway_identity.as_ref()
    }

    /// Signals shutdown and waits for the server thread to finish,
    /// reporting how the stop ended.
    ///
    /// The wait is bounded by the server's own watchdog: in-flight
    /// connections get a grace window to drain, and when one outlasts it
    /// (a request wedged mid-body), the runtime is torn down anyway and
    /// the stop reports [`Termination::Forced`].
    ///
    /// # Errors
    /// Returns `std::io::Error` if the server stopped with an error or the
    /// server thread panicked.
    pub fn shutdown(mut self) -> std::io::Result<Termination> {
        self.gateway.close_publication();
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        let stopped = self.stopped.recv();
        self.join_inner()?;
        // The barrier is sent on every post-readiness path, so a missing
        // report means the thread died without running its teardown.
        stopped.map_err(|_| std::io::Error::other("the server thread stopped without reporting"))
    }

    /// Waits for the server thread to finish on its own, without signaling
    /// shutdown.
    ///
    /// # Errors
    /// Returns `std::io::Error` if the server stopped with an error or the
    /// server thread panicked.
    pub fn join(mut self) -> std::io::Result<()> {
        self.join_inner()
    }

    fn join_inner(&mut self) -> std::io::Result<()> {
        let Some(thread) = self.thread.take() else {
            return Ok(());
        };
        match thread.join() {
            Ok(result) => result,
            Err(_) => Err(std::io::Error::other("workshop server thread panicked")),
        }
    }
}

impl Drop for ServerHandle {
    fn drop(&mut self) {
        self.gateway.close_publication();
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
    }
}

/// A failure to start the in-process workshop server: rich, init-only, and
/// never sent over the wire, so `#[from]` conveniences are welcome here.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SpawnError {
    /// The gateway endpoint could not be resolved, or the shared state
    /// could not be built.
    #[non_exhaustive]
    #[error("build shared state")]
    State(#[from] StateError),

    /// An I/O failure: the bind address is not loopback, the listener
    /// bind failed, the bound address could not be read, or the server
    /// thread could not be spawned.
    #[non_exhaustive]
    #[error("start workshop server")]
    Io(#[from] std::io::Error),
}

/// Spawns the workshop server on a dedicated thread and blocks until the
/// listener is bound.
///
/// The bound listener is the readiness signal: when this returns `Ok`, the
/// server is accepting connections at [`ServerHandle::url`].
///
/// # Errors
/// Returns [`SpawnError::State`] if the gateway endpoint cannot be
/// resolved (no live gateway discovery file and no explicit `[gateway]` config)
/// or the shared state cannot be built, and [`SpawnError::Io`] if the
/// bind address is not loopback, the bind fails, or the server thread
/// cannot be spawned.
pub fn spawn(config: Config) -> Result<ServerHandle, SpawnError> {
    spawn_inner(config, None, SHUTDOWN_GRACE)
}

#[cfg(feature = "test-fixtures")]
pub(crate) fn spawn_resolved(config: Config) -> Result<ServerHandle, SpawnError> {
    let gateway = ResolvedGateway::from_config(&config.gateway);
    spawn_inner(config, Some(gateway), SHUTDOWN_GRACE)
}

#[cfg(test)]
type TestTaskHook =
    Box<dyn FnOnce(&crate::app::AppState) -> Vec<workshop_registry::Registration> + Send>;
#[cfg(not(test))]
type TestTaskHook = ();

/// [`spawn`] with the shutdown grace window injectable, so tests prove the
/// forced path in milliseconds instead of waiting out the real window.
/// Discovery is bypassed, so a test never consults the real run directory.
#[cfg(test)]
fn spawn_with_grace(config: Config, grace: Duration) -> Result<ServerHandle, SpawnError> {
    let gateway = ResolvedGateway::from_config(&config.gateway);
    spawn_with_hook(config, Some(gateway), grace, None)
}

#[cfg(test)]
fn spawn_with_task(
    config: Config,
    grace: Duration,
    task: std::sync::Arc<dyn workshop_registry::BackgroundTask>,
) -> Result<ServerHandle, SpawnError> {
    let gateway = ResolvedGateway::from_config(&config.gateway);
    spawn_with_hook(
        config,
        Some(gateway),
        grace,
        Some(Box::new(move |state| {
            vec![state.registry().register_task(task)]
        })),
    )
}

fn spawn_inner(
    config: Config,
    gateway: Option<ResolvedGateway>,
    grace: Duration,
) -> Result<ServerHandle, SpawnError> {
    spawn_with_hook(config, gateway, grace, None)
}

fn spawn_with_hook(
    config: Config,
    gateway: Option<ResolvedGateway>,
    grace: Duration,
    test_hook: Option<TestTaskHook>,
) -> Result<ServerHandle, SpawnError> {
    // Composing state sweeps the state directory and reopening follows
    // the workspace pointer, so a refused address must fail before either.
    // `loopback_address` names the address in its message already.
    loopback_address(&config.server.bind).map_err(SpawnError::Io)?;
    // Discovery runs before the server thread starts: a resolution
    // failure is the plain no-gateway error, never a bind-then-fail.
    let gateway = match gateway {
        Some(gateway) => gateway,
        None => workshop_gateway::resolve(&config.gateway).map_err(StateError::Resolution)?,
    };
    let initial_gateway_identity = gateway.identity().cloned();
    let (ready_tx, ready_rx) = mpsc::channel();
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
    let (stopped_tx, stopped_rx) = mpsc::channel();
    let thread = std::thread::Builder::new()
        .name("workshop-server".to_string())
        .spawn(move || {
            serve_thread(
                config,
                gateway,
                ready_tx,
                shutdown_rx,
                &stopped_tx,
                (grace, test_hook),
            )
        })
        .map_err(|e| {
            SpawnError::Io(std::io::Error::new(
                e.kind(),
                format!("spawn the workshop-server thread: {e}"),
            ))
        })?;
    match ready_rx.recv() {
        Ok(Ok((url, config_origin, gateway))) => Ok(ServerHandle {
            url,
            config_origin,
            gateway,
            initial_gateway_identity,
            shutdown: Some(shutdown_tx),
            stopped: stopped_rx,
            thread: Some(thread),
        }),
        Ok(Err(error)) => {
            let _ = thread.join();
            Err(error)
        }
        Err(_) => {
            let _ = thread.join();
            Err(SpawnError::Io(std::io::Error::other(
                "workshop server thread exited before binding",
            )))
        }
    }
}

/// The server thread's body: build a runtime, build state, bind, reopen
/// the last workspace, signal readiness through `ready`, then serve until
/// `shutdown` resolves. When it does, a watchdog bounds the graceful drain
/// at `grace` before tearing the runtime down anyway, and `stopped` reports
/// which of the two endings ran.
///
/// Startup failures are reported through `ready`; only serving failures
/// become the thread's return value.
fn serve_thread(
    config: Config,
    gateway: ResolvedGateway,
    ready: mpsc::Sender<Result<(String, String, GatewayUpdater), SpawnError>>,
    shutdown: tokio::sync::oneshot::Receiver<()>,
    stopped: &mpsc::Sender<Termination>,
    (grace, test_hook): (Duration, Option<TestTaskHook>),
) -> std::io::Result<()> {
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            let _ = ready.send(Err(SpawnError::Io(error)));
            return Ok(());
        }
    };
    let (outcome, result) = runtime.block_on(async move {
        let state = match state_with_gateway(&config, &gateway) {
            Ok(state) => state,
            Err(error) => {
                let _ = ready.send(Err(SpawnError::State(error)));
                return (Termination::Graceful, Ok(()));
            }
        };
        #[cfg(test)]
        let _test_registrations = test_hook.map(|hook| hook(&state));
        #[cfg(not(test))]
        let _ = test_hook;
        let listener = match reuse_bind(&config.server.bind) {
            Ok(listener) => listener,
            Err(error) => {
                let _ = ready.send(Err(SpawnError::Io(error)));
                return (Termination::Graceful, Ok(()));
            }
        };
        let address = match listener.local_addr() {
            Ok(address) => address,
            Err(error) => return (Termination::Graceful, Err(error)),
        };
        let config_listener = match reuse_bind("127.0.0.1:0") {
            Ok(listener) => listener,
            Err(error) => {
                let _ = ready.send(Err(SpawnError::Io(error)));
                return (Termination::Graceful, Ok(()));
            }
        };
        let config_address = match config_listener.local_addr() {
            Ok(address) => address,
            Err(error) => return (Termination::Graceful, Err(error)),
        };
        let main_origin = format!("http://{address}");
        let config_origin = format!("http://{config_address}");
        state.set_config_origin(config_origin.clone());
        // The last-used workspace is restored after the bind, so a failed
        // bind never opens its file, and before readiness, so readiness
        // means the grants are already in place; a pointer that cannot be
        // followed starts ephemeral and never fails boot.
        state.reopen_last_workspace().await;
        let app = router(state.clone());
        let config_app = config_router(state.clone(), &main_origin);
        let _ = ready.send(Ok((main_origin, config_origin, state.gateway_updater())));
        // The subsystems' registered background tasks start with serving
        // and stop inside the same graceful-shutdown window, so they
        // never outlive the server.
        let tasks: Vec<_> = state
            .registry()
            .tasks()
            .iter()
            .map(|task| task.spawn())
            .collect();
        let outcome = serve_until_shutdown(
            (listener, app),
            (config_listener, config_app),
            shutdown,
            tasks,
            grace,
        )
        .await;
        state.close_gateway_publication();
        outcome
    });
    // The barrier reports before teardown: the embedding binary's join is
    // bounded by RUNTIME_TEARDOWN, not by what abandoned tasks still hold.
    let _ = stopped.send(outcome);
    // A graceful serve-return is not quiescence: sessions that detached at
    // upgrade can still be running, so both endings take the bounded
    // teardown - canceled tasks collapse at their next yield, and a wedged
    // blocking task is abandoned so it cannot park the embedding binary.
    // Never `process::exit` here - this crate runs inside embedding binaries.
    runtime.shutdown_timeout(RUNTIME_TEARDOWN);
    result
}

/// Drives both listeners alongside the registered-task drain so an idle
/// listener cannot report `Graceful` before cleanup finishes, while the
/// watchdog bounds HTTP draining and registered cleanup together.
async fn serve_until_shutdown(
    (listener, app): (tokio::net::TcpListener, axum::Router),
    (config_listener, config_app): (tokio::net::TcpListener, axum::Router),
    shutdown: tokio::sync::oneshot::Receiver<()>,
    tasks: Vec<workshop_registry::ShutdownHandle>,
    grace: Duration,
) -> (Termination, std::io::Result<()>) {
    let (draining_tx, draining_rx) = tokio::sync::oneshot::channel();
    let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
    let mut main_stop = stop_rx.clone();
    let mut config_stop = stop_rx;
    let main_serve = async {
        axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                let _ = main_stop.wait_for(|&stopped| stopped).await;
            })
            .await
    };
    let config_serve = async {
        axum::serve(config_listener, config_app)
            .with_graceful_shutdown(async move {
                let _ = config_stop.wait_for(|&stopped| stopped).await;
            })
            .await
    };
    let drain_tasks = async {
        let _ = shutdown.await;
        // Arm the watchdog before draining listeners and background
        // tasks, so the grace window bounds the whole stop.
        let _ = draining_tx.send(());
        let _ = stop_tx.send(true);
        for task in tasks {
            task.shutdown().await;
        }
    };
    let serve = async {
        tokio::pin!(main_serve, config_serve, drain_tasks);
        let mut main_res: Option<std::io::Result<()>> = None;
        let mut config_res: Option<std::io::Result<()>> = None;
        let mut drain_done = false;
        while !(main_res.is_some() && config_res.is_some() && drain_done) {
            tokio::select! {
                res = &mut main_serve, if main_res.is_none() => {
                    if res.is_err() && !*stop_tx.borrow() {
                        return res;
                    }
                    main_res = Some(res);
                }
                res = &mut config_serve, if config_res.is_none() => {
                    if res.is_err() && !*stop_tx.borrow() {
                        return res;
                    }
                    config_res = Some(res);
                }
                () = &mut drain_tasks, if !drain_done => {
                    drain_done = true;
                }
            }
        }
        match (main_res, config_res) {
            (Some(main), Some(config)) => main.and(config),
            (Some(res), None) | (None, Some(res)) => res,
            (None, None) => Ok(()),
        }
    };
    // axum's graceful shutdown waits for every connection it still
    // tracks - a request wedged mid-body parks it, while WebSockets
    // detach at upgrade (both pinned by the shutdown tests); once the
    // window lapses the watchdog wins the select and the graceful
    // wait (including any stalled cleanup task) is abandoned.
    let watchdog = async {
        let _ = draining_rx.await;
        tokio::time::sleep(grace).await;
    };
    tokio::select! {
        result = serve => (Termination::Graceful, result),
        () = watchdog => (Termination::Forced, Ok(())),
    }
}

/// Parses `address` as a socket address and refuses any that is not
/// loopback, with [`std::io::ErrorKind::InvalidInput`] either way.
fn loopback_address(address: &str) -> std::io::Result<std::net::SocketAddr> {
    let addr: std::net::SocketAddr = address.parse().map_err(|e| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("parse bind address {address}: {e}"),
        )
    })?;
    if !addr.ip().is_loopback() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("refusing to bind {addr}: the workshop server binds only to loopback"),
        ));
    }
    Ok(addr)
}

/// Binds a TCP listener with `SO_REUSEADDR` so a restart doesn't fail on
/// TIME_WAIT sockets from the previous instance.
fn reuse_bind(address: &str) -> std::io::Result<tokio::net::TcpListener> {
    // A second line of defense behind `spawn_inner`'s check: the socket
    // itself never binds a non-loopback address.
    let addr = loopback_address(address)?;
    let socket = socket2::Socket::new(
        socket2::Domain::for_address(addr),
        socket2::Type::STREAM,
        Some(socket2::Protocol::TCP),
    )?;
    socket.set_reuse_address(true)?;
    socket.set_nonblocking(true)?;
    socket.bind(&addr.into())?;
    socket.listen(1024)?;
    tokio::net::TcpListener::from_std(socket.into())
}

#[cfg(test)]
#[path = "serve-tests.rs"]
mod tests;
