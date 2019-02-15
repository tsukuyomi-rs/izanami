//! HTTP server.

use {
    crate::{
        net::{Bind, Listener},
        service::{HttpService, MakeContext, MakeHttpService, RequestBody, ResponseBody},
        system::{CurrentThread, DefaultRuntime, Runtime, Spawn, System},
        tls::{NoTls, TlsConfig, TlsWrapper},
    },
    futures::{future::Shared, Async, Future, IntoFuture, Poll, Stream},
    http::{Request, Response},
    izanami_util::RemoteAddr,
    tokio::sync::{mpsc, oneshot},
};

// FIXME: replace with never type.
enum Never {}

type SpawnFn<Rt, F> = dyn for<'s> FnMut(
        &mut System<'s, Rt>,
        &F,
        &ShutdownSignal,
        &mpsc::Sender<Never>,
    ) -> crate::Result<()>
    + 'static;

#[derive(Debug, Copy, Clone)]
enum HttpVersions {
    Http1Only,
    Http2Only,
    Fallback,
}

/// An HTTP server running on `System`.
#[allow(missing_debug_implementations)]
pub struct HttpServer<F, Rt>
where
    Rt: Runtime,
{
    service_factory: F,
    tx_shutdown: oneshot::Sender<()>,
    rx_shutdown: ShutdownSignal,
    spawn_fns: Vec<Box<SpawnFn<Rt, F>>>,
    http_versions: HttpVersions,
    alpn_protocols: Option<Vec<String>>,
}

impl<F, S, Rt> HttpServer<F, Rt>
where
    F: Fn() -> S,
    S: MakeHttpService,
    Rt: Runtime,
{
    /// Creates a `HttpServer` using the specified service factory.
    pub fn new(service_factory: F) -> Self {
        let (tx_shutdown, rx_shutdown) = oneshot::channel();
        Self {
            service_factory,
            tx_shutdown,
            rx_shutdown: ShutdownSignal {
                inner: Some(rx_shutdown.shared()),
            },
            spawn_fns: vec![],
            http_versions: HttpVersions::Fallback,
            alpn_protocols: None,
        }
    }

    /// Sets whether to use HTTP/1 is required or not.
    ///
    /// By default, the server try HTTP/2 at first and switches to HTTP/1 if failed.
    pub fn http1_only(self) -> Self {
        Self {
            http_versions: HttpVersions::Http1Only,
            ..self
        }
    }

    /// Sets whether to use HTTP/2 is required or not.
    ///
    /// By default, the server try HTTP/2 at first and switches to HTTP/1 if failed.
    pub fn http2_only(self) -> Self {
        Self {
            http_versions: HttpVersions::Http2Only,
            ..self
        }
    }

    /// Specifies the list of protocols used by ALPN.
    pub fn alpn_protocols(self, protocols: Vec<String>) -> Self {
        Self {
            alpn_protocols: Some(protocols),
            ..self
        }
    }

    /// Associate the specified I/O with this server.
    pub fn bind<B>(self, bind: B) -> crate::Result<Self>
    where
        B: Bind + 'static,
        HttpServerTask<S, B::Listener, NoTls>: Spawn<Rt>,
    {
        self.bind_tls(bind, NoTls::default())
    }

    /// Associate the specified I/O and SSL/TLS configuration with this server.
    pub fn bind_tls<B, T>(mut self, bind: B, tls: T) -> crate::Result<Self>
    where
        B: Bind + 'static,
        T: TlsConfig<B::Conn> + 'static,
        HttpServerTask<S, B::Listener, T::Wrapper>: Spawn<Rt>,
    {
        let mut listeners = bind.into_listeners()?;
        let http_versions = self.http_versions;

        let alpn_protocols = match (&self.alpn_protocols, http_versions) {
            (Some(protocols), _) => protocols.clone(),
            (None, HttpVersions::Http1Only) => vec!["http/1.1".into()],
            (None, HttpVersions::Http2Only) => vec!["h2".into()],
            (None, HttpVersions::Fallback) => vec!["h2".into(), "http/1.1".into()],
        };
        let tls_wrapper = tls.into_wrapper(alpn_protocols)?;

        self.spawn_fns.push(Box::new(
            move |sys, service_factory, rx_shutdown, tx_drained| {
                for listener in listeners.drain(..) {
                    sys.spawn(HttpServerTask {
                        listener,
                        tls_wrapper: tls_wrapper.clone(),
                        make_service: (*service_factory)(),
                        rx_shutdown: rx_shutdown.clone(),
                        tx_drained: tx_drained.clone(),
                        http_versions,
                    });
                }
                Ok(())
            },
        ));

        Ok(self)
    }

    /// Start all tasks registered on this server onto the specified system.
    ///
    /// This method returns a `Handle` to notify a shutdown signal to the all of
    /// spawned tasks and track their completion.
    pub fn start(self, sys: &mut System<'_, Rt>) -> crate::Result<Handle> {
        let (tx_drained, rx_drained) = mpsc::channel(1);
        for mut spawn_fn in self.spawn_fns {
            (spawn_fn)(sys, &self.service_factory, &self.rx_shutdown, &tx_drained)?;
        }

        Ok(Handle {
            tx_shutdown: self.tx_shutdown,
            draining: Draining { rx_drained },
        })
    }

    /// Run this server onto the specified `System`.
    pub fn run(self, sys: &mut System<'_, Rt>) -> crate::Result<()>
    where
        Handle: crate::system::BlockOn<Rt>,
    {
        let server = self.start(sys)?;
        let _ = sys.block_on(server);
        Ok(())
    }
}

#[derive(Debug)]
pub struct Handle {
    tx_shutdown: oneshot::Sender<()>,
    draining: Draining,
}

impl Handle {
    /// Send a shutdown signal to the background tasks.
    pub fn shutdown(self) -> impl Future<Item = (), Error = ()> {
        let _ = self.tx_shutdown.send(());
        self.draining
    }
}

impl Future for Handle {
    type Item = ();
    type Error = ();

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        self.draining.poll()
    }
}

#[derive(Debug)]
struct Draining {
    rx_drained: mpsc::Receiver<Never>,
}

impl Future for Draining {
    type Item = ();
    type Error = ();

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        match self.rx_drained.poll() {
            Ok(Async::Ready(Some(..))) | Err(..) => {
                unreachable!("Receiver<Never> never receives a value")
            }
            Ok(Async::Ready(None)) => Ok(Async::Ready(())),
            Ok(Async::NotReady) => Ok(Async::NotReady),
        }
    }
}

#[allow(missing_debug_implementations)]
#[derive(Clone)]
struct ShutdownSignal {
    inner: Option<futures::future::Shared<oneshot::Receiver<()>>>,
}

impl Future for ShutdownSignal {
    type Item = ();
    type Error = ();

    #[inline]
    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        match self.inner.as_mut().map(|inner| inner.poll()) {
            Some(Ok(Async::Ready(..))) => Ok(Async::Ready(())),
            Some(Ok(Async::NotReady)) => Ok(Async::NotReady),
            Some(Err(..)) => {
                log::trace!("the shutdown signal throws an error");
                log::trace!("  --> transit to the infinity run");
                drop(self.inner.take());
                Ok(Async::NotReady)
            }
            None => Ok(Async::NotReady),
        }
    }
}

trait TaskExecutor<F: Future<Item = (), Error = ()> + 'static> {
    fn spawn(&mut self, future: F);
}

impl<F> TaskExecutor<F> for tokio::runtime::TaskExecutor
where
    F: Future<Item = (), Error = ()> + Send + 'static,
{
    fn spawn(&mut self, future: F) {
        tokio::executor::Executor::spawn(self, Box::new(future))
            .unwrap_or_else(|e| log::error!("failed to spawn a task: {}", e))
    }
}

impl<F> TaskExecutor<F> for tokio::runtime::current_thread::TaskExecutor
where
    F: Future<Item = (), Error = ()> + 'static,
{
    fn spawn(&mut self, future: F) {
        self.spawn_local(Box::new(future))
            .unwrap_or_else(|e| log::error!("failed to spawn a task: {}", e))
    }
}

#[allow(unused)]
struct Graceful<L, E, F> {
    state: GracefulState<L, E, F>,
    tx_drained: Option<mpsc::Sender<Never>>,
}

enum GracefulState<L, E, F> {
    Running {
        signal: Option<Signal>,
        watch: Watch,
        listener: L,
        executor: E,
        serve_connection_fn: F,
        rx_shutdown: ShutdownSignal,
    },
    Draining(mpsc::Receiver<Never>),
}

impl<L, E, F, Fut> Future for Graceful<L, E, F>
where
    L: Listener,
    E: TaskExecutor<Fut>,
    F: FnMut(L::Conn, RemoteAddr, Watch) -> Fut,
    Fut: Future<Item = (), Error = ()> + 'static,
{
    type Item = ();
    type Error = ();

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        loop {
            self.state = match &mut self.state {
                GracefulState::Running {
                    signal,
                    watch,
                    listener,
                    executor,
                    serve_connection_fn,
                    rx_shutdown,
                    ..
                } => match rx_shutdown.poll() {
                    Ok(Async::Ready(())) | Err(..) => {
                        // start graceful shutdown
                        let signal = signal.take().unwrap();
                        let _ = signal.tx.send(());
                        GracefulState::Draining(signal.rx_drained)
                    }
                    Ok(Async::NotReady) => {
                        let (conn, addr) = match listener.poll_incoming() {
                            Ok(Async::Ready((conn, addr))) => (conn, addr),
                            Ok(Async::NotReady) => return Ok(Async::NotReady),
                            Err(e) => {
                                log::error!("accept error: {}", e);
                                drop(self.tx_drained.take());
                                return Ok(Async::Ready(()));
                            }
                        };
                        let future = serve_connection_fn(conn, addr, watch.clone());
                        executor.spawn(future);
                        continue;
                    }
                },
                GracefulState::Draining(rx_drained) => match rx_drained.poll() {
                    Ok(Async::Ready(None)) => {
                        drop(self.tx_drained.take());
                        return Ok(Async::Ready(()));
                    }
                    Ok(Async::Ready(Some(..))) | Err(..) => {
                        unreachable!("rx_drain never receives a value")
                    }
                    Ok(Async::NotReady) => return Ok(Async::NotReady),
                },
            };
        }
    }
}

struct Signal {
    tx: oneshot::Sender<()>,
    rx_drained: mpsc::Receiver<Never>,
}

#[derive(Clone)]
#[allow(missing_debug_implementations)]
struct Watch {
    rx: Shared<oneshot::Receiver<()>>,
    tx_drained: mpsc::Sender<Never>,
}

impl Watch {
    fn watch(&mut self) -> bool {
        match self.rx.poll() {
            Ok(Async::Ready(..)) | Err(..) => true,
            Ok(Async::NotReady) => false,
        }
    }
}

#[allow(missing_debug_implementations)]
struct Watching<Fut, F>
where
    Fut: Future,
    F: FnOnce(&mut Fut),
{
    future: Fut,
    on_drain: Option<F>,
    watch: Watch,
}

impl<Fut, F> Watching<Fut, F>
where
    Fut: Future,
    F: FnOnce(&mut Fut),
{
    fn new(watch: Watch, future: Fut, on_drain: F) -> Self {
        Self {
            future,
            on_drain: Some(on_drain),
            watch,
        }
    }
}

impl<Fut, F> Future for Watching<Fut, F>
where
    Fut: Future,
    F: FnOnce(&mut Fut),
{
    type Item = Fut::Item;
    type Error = Fut::Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        loop {
            match self.on_drain.take() {
                Some(on_drain) => {
                    if self.watch.watch() {
                        on_drain(&mut self.future);
                        continue;
                    } else {
                        self.on_drain = Some(on_drain);
                        return self.future.poll();
                    }
                }
                None => return self.future.poll(),
            }
        }
    }
}

#[doc(hidden)]
#[allow(missing_debug_implementations)]
pub struct HttpServerTask<S, L, T> {
    make_service: S,
    listener: L,
    tls_wrapper: T,
    rx_shutdown: ShutdownSignal,
    tx_drained: mpsc::Sender<Never>,
    http_versions: HttpVersions,
}

/// A helper macro for creating a server task.
// TODOs:
// * add SNI support
// * refactor
macro_rules! spawn_inner {
    ($self:expr, $runtime:expr) => {{
        let self_ = $self;
        let runtime = $runtime;
        let (tx, rx) = oneshot::channel();
        // tokio_sync::mpsc::channel requires that the capacity is greater at least one.
        let (tx_drained, rx_drained) = mpsc::channel(1);
        runtime.spawn(Graceful {
            tx_drained: Some(self_.tx_drained),
            state: GracefulState::Running {
                signal: Some(Signal { tx, rx_drained }),
                watch: Watch {
                    rx: rx.shared(),
                    tx_drained,
                },
                listener: self_.listener,
                executor: runtime.executor(),
                rx_shutdown: self_.rx_shutdown,
                serve_connection_fn: {
                    let mut make_service = self_.make_service;
                    let mut executor = runtime.executor();
                    let tls_wrapper = self_.tls_wrapper;

                    let mut protocol =
                        hyper::server::conn::Http::new().with_executor(executor.clone());
                    match self_.http_versions {
                        HttpVersions::Http1Only => {
                            protocol.http1_only(true);
                        }
                        HttpVersions::Http2Only => {
                            protocol.http2_only(true);
                        }
                        _ => {}
                    }

                    move |conn, remote_addr, watch| {
                        let mk_svc_fut = make_service
                            .make_service(&mut MakeContext::new(&remote_addr))
                            .map_err(|e| log::error!("make service error: {}", e.into()));
                        let mk_conn_fut = tls_wrapper
                            .wrap(conn)
                            .map_err(|e| log::error!("tls error: {}", e.into()));
                        let protocol = protocol.clone();
                        (mk_svc_fut, mk_conn_fut) //
                            .into_future()
                            .and_then(move |(service, conn)| {
                                // FIXME: server name indication.
                                Watching::new(
                                    watch,
                                    protocol
                                        .serve_connection(
                                            conn,
                                            IzanamiService {
                                                service,
                                                remote_addr,
                                            },
                                        )
                                        .with_upgrades(),
                                    |conn| conn.graceful_shutdown(),
                                )
                                .map_err(|e| log::error!("connection error: {}", e))
                            })
                    }
                },
            },
        });
    }};
}

impl<S, L, T> Spawn for HttpServerTask<S, L, T>
where
    S: MakeHttpService + Send + Sync + 'static,
    S::Future: Send + 'static,
    S::Service: Send + 'static,
    <S::Service as HttpService>::Future: Send + 'static,
    L: Listener + Send + 'static,
    T: TlsWrapper<L::Conn> + Send + 'static,
    T::Wrapped: Send + 'static,
    T::Future: Send + 'static,
{
    #[allow(unused_mut)]
    fn spawn(self, rt: &mut DefaultRuntime) {
        spawn_inner!(self, rt)
    }
}

impl<S, L, T> Spawn<CurrentThread> for HttpServerTask<S, L, T>
where
    S: MakeHttpService + 'static,
    S::Service: 'static,
    S::Future: 'static,
    L: Listener + 'static,
    T: TlsWrapper<L::Conn> + 'static,
    T::Wrapped: Send + 'static,
    T::Future: 'static,
{
    #[allow(unused_mut)]
    fn spawn(self, rt: &mut CurrentThread) {
        spawn_inner!(self, rt)
    }
}

#[allow(missing_debug_implementations)]
struct IzanamiService<S> {
    service: S,
    remote_addr: RemoteAddr,
}

impl<S> hyper::service::Service for IzanamiService<S>
where
    S: HttpService,
{
    type ReqBody = hyper::Body;
    type ResBody = InnerBody<S::ResponseBody>;
    type Error = S::Error;
    type Future = LiftedHttpServiceFuture<S::Future>;

    #[inline]
    fn call(&mut self, request: Request<hyper::Body>) -> Self::Future {
        let mut request = request.map(RequestBody::from_hyp);
        request.extensions_mut().insert(self.remote_addr.clone());

        LiftedHttpServiceFuture(self.service.call(request))
    }
}

#[allow(missing_debug_implementations)]
struct LiftedHttpServiceFuture<Fut>(Fut);

impl<Fut, Bd> Future for LiftedHttpServiceFuture<Fut>
where
    Fut: Future<Item = Response<Bd>>,
    Bd: ResponseBody + Send + 'static,
{
    type Item = Response<InnerBody<Bd>>;
    type Error = Fut::Error;

    #[inline]
    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        self.0
            .poll()
            .map(|x| x.map(|response| response.map(InnerBody)))
    }
}

#[allow(missing_debug_implementations)]
struct InnerBody<Bd>(Bd);

impl<Bd> hyper::body::Payload for InnerBody<Bd>
where
    Bd: ResponseBody + Send + 'static,
{
    type Data = Bd::Item;
    type Error = crate::error::BoxedStdError;

    fn poll_data(&mut self) -> Poll<Option<Self::Data>, Self::Error> {
        self.0.poll_buf().map_err(Into::into)
    }

    fn poll_trailers(&mut self) -> Poll<Option<http::HeaderMap>, Self::Error> {
        self.0.poll_trailers().map_err(Into::into)
    }

    fn content_length(&self) -> Option<u64> {
        let hint = self.0.size_hint();
        match (hint.lower(), hint.upper()) {
            (lower, Some(upper)) if lower == upper => Some(upper),
            _ => None,
        }
    }
}
