//! HTTP server.

use {
    crate::error::BoxedStdError,
    crate::{
        net::{Bind, Listener},
        system::{notify, CurrentThread, DefaultRuntime, Runtime, Spawn, System},
        tls::{NoTls, TlsConfig, TlsWrapper},
    },
    bytes::{Buf, BufMut, Bytes},
    futures::{future::Shared, Async, Future, IntoFuture, Poll, Stream},
    http::{HeaderMap, Request, Response},
    hyper::body::Payload as _Payload,
    izanami_service::{MakeService, Service},
    izanami_util::{
        buf_stream::{BufStream, SizeHint},
        http::{HasTrailers, Upgrade},
        RemoteAddr,
    },
    std::{io, marker::PhantomData},
    tokio::{
        io::{AsyncRead, AsyncWrite},
        sync::{mpsc, oneshot},
    },
};

type SpawnFn<Rt, F> = dyn for<'s> FnMut(
        &mut System<'s, Rt>,
        &F,
        &ShutdownSignal,
        &mut Vec<crate::system::Handle<'s, Rt, crate::Result<()>>>,
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
        HttpServerTask<S, B::Listener, NoTls>: Spawn<Rt, Output = crate::Result<()>>,
    {
        self.bind_tls(bind, NoTls::default())
    }

    /// Associate the specified I/O and SSL/TLS configuration with this server.
    pub fn bind_tls<B, T>(mut self, bind: B, tls: T) -> crate::Result<Self>
    where
        B: Bind + 'static,
        T: TlsConfig<B::Conn> + 'static,
        HttpServerTask<S, B::Listener, T::Wrapper>: Spawn<Rt, Output = crate::Result<()>>,
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
            move |sys, service_factory, rx_shutdown, handles| {
                for listener in listeners.drain(..) {
                    let handle = sys.spawn(HttpServerTask {
                        listener,
                        tls_wrapper: tls_wrapper.clone(),
                        make_service: (*service_factory)(),
                        rx_shutdown: rx_shutdown.clone(),
                        http_versions,
                    });
                    handles.push(handle);
                }
                Ok(())
            },
        ));

        Ok(self)
    }

    /// Start all tasks registered on this server and returns a handle that controls their tasks.
    pub fn start<'s>(self, sys: &mut System<'s, Rt>) -> crate::Result<ServeHandle<'s, Rt>> {
        let mut handles = vec![];
        for mut spawn_fn in self.spawn_fns {
            (spawn_fn)(sys, &self.service_factory, &self.rx_shutdown, &mut handles)?;
        }

        Ok(ServeHandle {
            handles,
            tx_shutdown: Some(self.tx_shutdown),
        })
    }
}

impl<F, S> HttpServer<F, DefaultRuntime>
where
    F: Fn() -> S,
    S: MakeHttpService,
{
    /// Create a `System` using the default runtime and drive this serve within it.
    pub fn run(self) -> crate::Result<()> {
        crate::system::run(move |sys| self.start(sys)?.wait_complete(sys))
    }
}

impl<F, S> HttpServer<F, CurrentThread>
where
    F: Fn() -> S,
    S: MakeHttpService,
{
    /// Create a `System` using the single-threaded runtime and drive this serve within it.
    pub fn run_local(self) -> crate::Result<()> {
        crate::system::run_local(move |sys| self.start(sys)?.wait_complete(sys))
    }
}

#[derive(Debug)]
pub struct ServeHandle<'s, Rt: Runtime> {
    handles: Vec<crate::system::Handle<'s, Rt, crate::Result<()>>>,
    tx_shutdown: Option<oneshot::Sender<()>>,
}

impl<'s, Rt: Runtime> ServeHandle<'s, Rt> {
    pub fn send_shutdown_signal(&mut self) {
        if let Some(tx_shutdown) = self.tx_shutdown.take() {
            let _ = tx_shutdown.send(());
        }
    }

    pub fn wait_complete(self, sys: &mut System<'s, Rt>) -> crate::Result<()>
    where
        crate::system::notify::Receiver<crate::Result<()>>:
            crate::system::BlockOn<Rt, Output = Result<crate::Result<()>, ()>>,
    {
        for handle in self.handles {
            handle.wait_complete(sys)?;
        }
        Ok(())
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
    tx_notify: Option<notify::Sender<crate::Result<()>>>,
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
                                self.tx_notify.take().unwrap().send(Err(e.into()));
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
                        self.tx_notify.take().unwrap().send(Ok(()));
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

enum Never {}

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
        let (tx_notify, rx_notify) = notify::pair();
        let (tx, rx) = oneshot::channel();
        // tokio_sync::mpsc::channel requires that the capacity is greater at least one.
        let (tx_drained, rx_drained) = mpsc::channel(1);
        runtime.spawn(Graceful {
            tx_notify: Some(tx_notify),
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
                            .make_service(&mut Context::new(&remote_addr))
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
        rx_notify
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
    type Output = crate::Result<()>;

    #[allow(unused_mut)]
    fn spawn(self, rt: &mut DefaultRuntime) -> notify::Receiver<Self::Output> {
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
    type Output = crate::Result<()>;

    #[allow(unused_mut)]
    fn spawn(self, rt: &mut CurrentThread) -> notify::Receiver<Self::Output> {
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

// ==== Service ====

/// Context values passed from the server when creating the service.
///
/// Currently, there is nothing available from the value of this type.
#[derive(Debug)]
pub struct Context<'a> {
    remote_addr: &'a RemoteAddr,
    _anchor: PhantomData<std::rc::Rc<()>>,
}

impl<'a> Context<'a> {
    pub(crate) fn new(remote_addr: &'a RemoteAddr) -> Self {
        Self {
            remote_addr,
            _anchor: PhantomData,
        }
    }

    pub fn remote_addr(&self) -> &RemoteAddr {
        &*self.remote_addr
    }
}

/// An asynchronous stream of chunks that represents the HTTP request body.
#[derive(Debug)]
pub struct RequestBody(Inner);

#[derive(Debug)]
enum Inner {
    Stream(hyper::Body),
    OnUpgrade(hyper::upgrade::OnUpgrade),
}

impl RequestBody {
    pub(crate) fn from_hyp(body: hyper::Body) -> Self {
        RequestBody(Inner::Stream(body))
    }

    /// Returns whether the body is complete or not.
    pub fn is_end_stream(&self) -> bool {
        match &self.0 {
            Inner::Stream(body) => body.is_end_stream(),
            _ => true,
        }
    }

    /// Returns whether this stream has already been upgraded.
    ///
    /// If this method returns `true`, the result from `BufStream::poll_buf`
    /// or `HasTrailers::poll_trailers` becomes an error.
    pub fn is_upgraded(&self) -> bool {
        match self.0 {
            Inner::OnUpgrade(..) => true,
            _ => false,
        }
    }

    /// Returns a length of the total bytes, if possible.
    pub fn content_length(&self) -> Option<u64> {
        match &self.0 {
            Inner::Stream(body) => body.content_length(),
            _ => None,
        }
    }
}

impl BufStream for RequestBody {
    type Item = io::Cursor<Bytes>;
    type Error = BoxedStdError;

    fn poll_buf(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        match &mut self.0 {
            Inner::Stream(body) => body
                .poll_data()
                .map(|x| x.map(|opt| opt.map(|chunk| io::Cursor::new(chunk.into_bytes()))))
                .map_err(Into::into),
            Inner::OnUpgrade(..) => Err(already_upgraded()),
        }
    }

    fn size_hint(&self) -> SizeHint {
        match &self.0 {
            Inner::Stream(body) => {
                let mut hint = SizeHint::new();
                if let Some(len) = body.content_length() {
                    hint.set_upper(len);
                    hint.set_lower(len);
                }
                hint
            }
            Inner::OnUpgrade(..) => SizeHint::new(),
        }
    }
}

impl HasTrailers for RequestBody {
    type TrailersError = BoxedStdError;

    fn poll_trailers(&mut self) -> Poll<Option<HeaderMap>, Self::TrailersError> {
        match &mut self.0 {
            Inner::Stream(body) => body.poll_trailers().map_err(Into::into),
            Inner::OnUpgrade(..) => Err(already_upgraded()),
        }
    }
}

impl Upgrade for RequestBody {
    type Upgraded = Upgraded;
    type Error = BoxedStdError;

    fn poll_upgrade(&mut self) -> Poll<Self::Upgraded, Self::Error> {
        loop {
            self.0 = match &mut self.0 {
                Inner::Stream(body) => {
                    let body = std::mem::replace(body, hyper::Body::empty());
                    Inner::OnUpgrade(body.on_upgrade())
                }
                Inner::OnUpgrade(on_upgrade) => {
                    return on_upgrade
                        .poll()
                        .map(|x| x.map(Upgraded))
                        .map_err(Into::into);
                }
            };
        }
    }
}

#[derive(Debug)]
pub struct Upgraded(hyper::upgrade::Upgraded);

impl Upgraded {
    #[inline]
    pub fn downcast<T>(self) -> Result<(T, Bytes), Self>
    where
        T: AsyncRead + AsyncWrite + 'static,
    {
        self.0
            .downcast::<T>()
            .map(|parts| (parts.io, parts.read_buf))
            .map_err(Upgraded)
    }
}

impl io::Read for Upgraded {
    #[inline]
    fn read(&mut self, dst: &mut [u8]) -> io::Result<usize> {
        self.0.read(dst)
    }
}

impl io::Write for Upgraded {
    #[inline]
    fn write(&mut self, src: &[u8]) -> io::Result<usize> {
        self.0.write(src)
    }

    #[inline]
    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}

impl AsyncRead for Upgraded {
    #[inline]
    unsafe fn prepare_uninitialized_buffer(&self, buf: &mut [u8]) -> bool {
        self.0.prepare_uninitialized_buffer(buf)
    }

    #[inline]
    fn read_buf<B: BufMut>(&mut self, buf: &mut B) -> Poll<usize, io::Error> {
        self.0.read_buf(buf)
    }
}

impl AsyncWrite for Upgraded {
    #[inline]
    fn write_buf<B: Buf>(&mut self, buf: &mut B) -> Poll<usize, io::Error> {
        self.0.write_buf(buf)
    }

    #[inline]
    fn shutdown(&mut self) -> Poll<(), io::Error> {
        self.0.shutdown()
    }
}

pub trait MakeHttpService {
    type ResponseBody: ResponseBody + Send + 'static;
    type Error: Into<BoxedStdError>;
    type Service: HttpService<ResponseBody = Self::ResponseBody, Error = Self::Error>;
    type MakeError: Into<BoxedStdError>;
    type Future: Future<Item = Self::Service, Error = Self::MakeError>;

    fn make_service(&mut self, cx: &mut Context<'_>) -> Self::Future;
}

impl<S, Bd, SvcErr, MkErr, Svc, Fut> MakeHttpService for S
where
    S: for<'cx, 'srv> MakeService<
        &'cx mut Context<'srv>, //
        Request<RequestBody>,
        Response = Response<Bd>,
        Error = SvcErr,
        Service = Svc,
        MakeError = MkErr,
        Future = Fut,
    >,
    SvcErr: Into<BoxedStdError>,
    MkErr: Into<BoxedStdError>,
    Svc: Service<Request<RequestBody>, Response = Response<Bd>, Error = SvcErr>,
    Fut: Future<Item = Svc, Error = MkErr>,
    Bd: ResponseBody + Send + 'static,
{
    type ResponseBody = Bd;
    type Error = SvcErr;
    type Service = Svc;
    type MakeError = MkErr;
    type Future = Fut;

    fn make_service(&mut self, cx: &mut Context<'_>) -> Self::Future {
        MakeService::make_service(self, cx)
    }
}

pub trait HttpService {
    type ResponseBody: ResponseBody + Send + 'static;
    type Error: Into<BoxedStdError>;
    type Future: Future<Item = Response<Self::ResponseBody>, Error = Self::Error>;

    fn call(&mut self, request: Request<RequestBody>) -> Self::Future;
}

impl<S, Bd> HttpService for S
where
    S: Service<Request<RequestBody>, Response = Response<Bd>>,
    S::Error: Into<BoxedStdError>,
    Bd: ResponseBody + Send + 'static,
{
    type ResponseBody = Bd;
    type Error = S::Error;
    type Future = S::Future;

    fn call(&mut self, request: Request<RequestBody>) -> Self::Future {
        Service::call(self, request)
    }
}

pub trait ResponseBody {
    type Item: Buf + Send;
    type Error: Into<BoxedStdError>;
    type TrailersError: Into<BoxedStdError>;

    fn poll_buf(&mut self) -> Poll<Option<Self::Item>, Self::Error>;

    fn poll_trailers(&mut self) -> Poll<Option<HeaderMap>, Self::TrailersError>;

    fn size_hint(&self) -> SizeHint;
}

impl<Bd> ResponseBody for Bd
where
    Bd: BufStream + HasTrailers,
    Bd::Item: Send,
    Bd::Error: Into<BoxedStdError>,
    Bd::TrailersError: Into<BoxedStdError>,
{
    type Item = Bd::Item;
    type Error = Bd::Error;
    type TrailersError = Bd::TrailersError;

    #[inline]
    fn poll_buf(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        BufStream::poll_buf(self)
    }

    #[inline]
    fn poll_trailers(&mut self) -> Poll<Option<HeaderMap>, Self::TrailersError> {
        HasTrailers::poll_trailers(self)
    }

    #[inline]
    fn size_hint(&self) -> SizeHint {
        BufStream::size_hint(self)
    }
}

fn already_upgraded() -> BoxedStdError {
    failure::format_err!("the request body has already been upgraded")
        .compat()
        .into()
}
