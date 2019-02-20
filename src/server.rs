//! The implementation of HTTP server.

use {
    crate::{
        drain::{Signal, Watch},
        error::BoxedStdError,
        service::{
            imp::{HttpResponseImpl, ResponseBodyImpl},
            HttpService, IntoHttpService, MakeHttpService, RequestBody,
        },
        tls::{MakeTlsTransport, NoTls},
        util::*,
    },
    futures::{future::Executor, Async, Future, IntoFuture, Poll, Stream},
    http::{Request, Response},
    hyper::server::conn::Http,
    izanami_rt::{Runnable, Runtime, Spawn, Spawner},
    izanami_service::StreamService,
    std::io,
    tokio::io::{AsyncRead, AsyncWrite},
};

/// A builder for creating an `Server`.
#[derive(Debug)]
pub struct Builder<S, Sig = futures::future::Empty<(), ()>> {
    stream_service: S,
    protocol: Http,
    shutdown_signal: Sig,
}

impl<S, C, T, Sig> Builder<S, Sig>
where
    S: StreamService<Response = (C, T)>,
    C: HttpService,
    T: AsyncRead + AsyncWrite,
    Sig: Future<Item = ()>,
{
    /// Creates a `Builder` using a streamed service.
    pub fn new(stream_service: S, protocol: Http, shutdown_signal: Sig) -> Self {
        Self {
            stream_service,
            protocol,
            shutdown_signal,
        }
    }

    /// Specifies that the server uses only HTTP/1.
    pub fn http1_only(mut self) -> Self {
        self.protocol.http1_only(true);
        self
    }

    /// Specifies that the server uses only HTTP/2.
    pub fn http2_only(mut self) -> Self {
        self.protocol.http2_only(true);
        self
    }

    /// Specifies the signal to shutdown the background tasks gracefully.
    pub fn with_graceful_shutdown<Sig2>(self, signal: Sig2) -> Builder<S, Sig2>
    where
        Sig2: Future<Item = ()>,
    {
        Builder {
            stream_service: self.stream_service,
            protocol: self.protocol,
            shutdown_signal: signal,
        }
    }

    /// Consumes itself and create an instance of `Server`.
    ///
    /// It also returns a `Handle` used by controlling the spawned server.
    pub fn build(self) -> Server<S, Sig> {
        Server {
            stream_service: self.stream_service,
            protocol: self.protocol,
            shutdown_signal: self.shutdown_signal,
        }
    }
}

/// An HTTP server.
#[derive(Debug)]
pub struct Server<S, Sig = futures::future::Empty<(), ()>> {
    stream_service: S,
    protocol: Http,
    shutdown_signal: Sig,
}

impl<S, C, T> Server<S>
where
    S: StreamService<Response = (C, T)>,
    C: HttpService,
    T: AsyncRead + AsyncWrite,
{
    /// Creates a `Builder` using a streamed service.
    pub fn builder(stream_service: S) -> Builder<S> {
        Builder::new(stream_service, Http::new(), futures::future::empty())
    }
}

impl<S, C, T, Sig> Server<S, Sig>
where
    S: StreamService<Response = (C, T)>,
    C: HttpService,
    T: AsyncRead + AsyncWrite,
    Sig: Future<Item = ()>,
{
    #[doc(hidden)]
    pub fn get_ref(&self) -> &S {
        &self.stream_service
    }

    #[doc(hidden)]
    pub fn get_mut(&mut self) -> &mut S {
        &mut self.stream_service
    }

    /// Start this server onto the specified spawner.
    ///
    /// This method immediately returns and the server runs on the background.
    pub fn start<Sp>(self, spawner: &mut Sp)
    where
        Self: Spawn<Sp>,
        Sp: Spawner + ?Sized,
    {
        Spawn::spawn(self, spawner)
    }

    /// Run this server onto the specified runtime.
    ///
    /// This method runs the server without spawning, and will block the current thread.
    pub fn run<Rt>(self, runtime: &mut Rt) -> <Self as Runnable<Rt>>::Output
    where
        Self: Runnable<Rt>,
        Rt: Runtime + ?Sized,
    {
        Runnable::run(self, runtime)
    }
}

impl<S, T> Server<Incoming<S, crate::net::tcp::AddrIncoming, T>>
where
    S: MakeHttpService<crate::net::tcp::AddrStream, T::Transport>,
    T: MakeTlsTransport<crate::net::tcp::AddrStream>,
{
    /// Create a `Builder` bound to the specified address.
    pub fn bind_tcp<A>(
        make_service: S,
        addr: A,
        tls: T,
    ) -> io::Result<Builder<Incoming<S, crate::net::tcp::AddrIncoming, T>>>
    where
        A: std::net::ToSocketAddrs,
    {
        Ok(Server::builder(Incoming::bind_tcp(
            make_service,
            addr,
            tls,
        )?))
    }
}

#[cfg(unix)]
impl<S, T> Server<Incoming<S, crate::net::unix::AddrIncoming, T>>
where
    S: MakeHttpService<crate::net::unix::AddrStream, T::Transport>,
    T: MakeTlsTransport<crate::net::unix::AddrStream>,
{
    /// Create a `Builder` bound to the specified socket path.
    pub fn bind_unix<P>(
        make_service: S,
        path: P,
        tls: T,
    ) -> io::Result<Builder<Incoming<S, crate::net::unix::AddrIncoming, T>>>
    where
        P: AsRef<std::path::Path>,
    {
        Ok(Server::builder(Incoming::bind_unix(
            make_service,
            path,
            tls,
        )?))
    }
}

trait HasExecutor {
    type Executor;
    fn executor(&self) -> Self::Executor;
}

impl HasExecutor for tokio::runtime::Runtime {
    type Executor = tokio::runtime::TaskExecutor;
    fn executor(&self) -> Self::Executor {
        self.executor()
    }
}

impl HasExecutor for tokio::runtime::TaskExecutor {
    type Executor = Self;
    fn executor(&self) -> Self::Executor {
        self.clone()
    }
}

impl HasExecutor for tokio::executor::DefaultExecutor {
    type Executor = Self;
    fn executor(&self) -> Self::Executor {
        self.clone()
    }
}

impl HasExecutor for tokio::runtime::current_thread::Runtime {
    type Executor = tokio::runtime::current_thread::TaskExecutor;
    fn executor(&self) -> Self::Executor {
        tokio::runtime::current_thread::TaskExecutor::current()
    }
}

impl HasExecutor for tokio::runtime::current_thread::TaskExecutor {
    type Executor = Self;
    fn executor(&self) -> Self::Executor {
        self.clone()
    }
}

/// A helper macro for creating a server task.
macro_rules! spawn_all_task {
    (<$S:ty> $self:expr, $executor:expr) => {{
        let this = $self;
        let executor = $executor;

        let serve_connection_fn = {
            let protocol = this.protocol.with_executor(executor.clone());
            move |fut: <$S as StreamService>::Future, watch: Watch| {
                let protocol = protocol.clone();
                fut.map_err(|_e| log::error!("stream service error"))
                    .and_then(move |(service, stream)| {
                        let conn = protocol
                            .serve_connection(stream, InnerService(service))
                            .with_upgrades();
                        watch
                            .watching(conn, |c, _| c.poll(), |c| c.graceful_shutdown())
                            .map_err(|e| log::error!("connection error: {}", e))
                    })
            }
        };

        let (signal, watch) = crate::drain::channel();
        SpawnAll {
            state: SpawnAllState::Running {
                stream_service: this.stream_service,
                shutdown_signal: this.shutdown_signal,
                executor,
                signal: Some(signal),
                watch,
                serve_connection_fn,
            },
        }
    }};
}

macro_rules! impl_spawn_for_server {
    ($t:ty) => {
        impl_spawn_for_server!($t, (rt, task) => { rt.spawn(task); });
    };
    ($t:ty, ($rt:ident, $task:ident) => $e:expr) => {
        impl<S, C, T, Sig> Spawn<$t> for Server<S, Sig>
        where
            S: StreamService<Response = (C, T)> + Send + 'static,
            S::Future: Send + 'static,
            C: HttpService + Send + 'static,
            C::Future: Send + 'static,
            T: AsyncRead + AsyncWrite + Send + 'static,
            Sig: Future<Item = ()> + Send + 'static,
        {
            fn spawn(self, $rt: &mut $t) {
                let $task = spawn_all_task!(<S> self, $rt.executor())
                    .map_err(|_e| log::error!("stream service error"));
                $e;
            }
        }
    };

    (!Send $t:ty) => {
        impl_spawn_for_server!(!Send $t, (rt, task) => { rt.spawn(task); });
    };
    (!Send $t:ty, ($rt:ident, $task:ident) => $e:expr) => {
        impl<S, C, T, Sig> Spawn<$t> for Server<S, Sig>
        where
            S: StreamService<Response = (C, T)> + 'static,
            S::Future: 'static,
            C: HttpService + 'static,
            T: AsyncRead + AsyncWrite + Send + 'static,
            Sig: Future<Item = ()> + 'static,
        {
            fn spawn(self, $rt: &mut $t) {
                let $task = spawn_all_task!(<S> self, $rt.executor())
                    .map_err(|_e| log::error!("stream service error"));
                $e;
            }
        }
    };
}

impl_spawn_for_server!(tokio::runtime::Runtime);
impl_spawn_for_server!(tokio::runtime::TaskExecutor);
impl_spawn_for_server!(tokio::executor::DefaultExecutor,
    (spawner, task) => {
        use tokio::executor::Executor;
        spawner
            .spawn(Box::new(task))
            .expect("failed to spawn a task");
    }
);
impl_spawn_for_server!(!Send tokio::runtime::current_thread::Runtime);
impl_spawn_for_server!(!Send tokio::runtime::current_thread::TaskExecutor,
    (spawner, task) => {
        spawner
            .spawn_local(Box::new(task))
            .expect("failed to spawn a task");
    }
);

impl<S, C, T, Sig> Runnable<tokio::runtime::Runtime> for Server<S, Sig>
where
    S: StreamService<Response = (C, T)> + Send + 'static,
    S::Error: Send + 'static,
    S::Future: Send + 'static,
    C: HttpService + Send + 'static,
    C::Future: Send + 'static,
    T: AsyncRead + AsyncWrite + Send + 'static,
    Sig: Future<Item = ()> + Send + 'static,
{
    type Output = Result<(), S::Error>;

    fn run(self, rt: &mut tokio::runtime::Runtime) -> Self::Output {
        let task = spawn_all_task!(<S> self, rt.executor());
        rt.block_on(task)
    }
}

impl<S, C, T, Sig> Runnable<tokio::runtime::current_thread::Runtime> for Server<S, Sig>
where
    S: StreamService<Response = (C, T)>,
    S::Future: 'static,
    C: HttpService + 'static,
    T: AsyncRead + AsyncWrite + Send + 'static,
    Sig: Future<Item = ()>,
{
    type Output = Result<(), S::Error>;

    fn run(self, rt: &mut tokio::runtime::current_thread::Runtime) -> Self::Output {
        let task = spawn_all_task!(<S> self, rt.executor());
        rt.block_on(task)
    }
}

struct SpawnAll<S, Sig, F, E> {
    state: SpawnAllState<S, Sig, F, E>,
}

enum SpawnAllState<S, Sig, F, E> {
    Running {
        stream_service: S,
        shutdown_signal: Sig,
        serve_connection_fn: F,
        signal: Option<Signal>,
        watch: Watch,
        executor: E,
    },
    Done(crate::drain::Draining),
}

impl<S, C, T, Sig, F, Fut, E> Future for SpawnAll<S, Sig, F, E>
where
    S: StreamService<Response = (C, T)>,
    C: HttpService,
    T: AsyncRead + AsyncWrite + Send + 'static,
    Sig: Future<Item = ()>,
    F: FnMut(S::Future, Watch) -> Fut,
    Fut: Future<Item = (), Error = ()>,
    E: Executor<Fut>,
{
    type Item = ();
    type Error = S::Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        loop {
            self.state = match self.state {
                SpawnAllState::Running {
                    ref mut stream_service,
                    ref mut shutdown_signal,
                    ref mut serve_connection_fn,
                    ref mut signal,
                    ref watch,
                    ref executor,
                } => match shutdown_signal.poll() {
                    Ok(Async::Ready(())) | Err(..) => {
                        let signal = signal.take().expect("unexpected condition");
                        SpawnAllState::Done(signal.drain())
                    }
                    Ok(Async::NotReady) => {
                        match futures::try_ready!(stream_service.poll_next_service()) {
                            Some(fut) => {
                                let serve_connection = serve_connection_fn(fut, watch.clone());
                                executor
                                    .execute(serve_connection)
                                    .unwrap_or_else(|_e| log::error!("executor error"));
                                continue;
                            }
                            None => {
                                // should be wait for the background tasks to finish?
                                return Ok(Async::Ready(()));
                            }
                        }
                    }
                },
                SpawnAllState::Done(ref mut draining) => {
                    return draining
                        .poll()
                        .map_err(|_e| unreachable!("draining never fails"));
                }
            }
        }
    }
}

#[allow(missing_debug_implementations)]
struct InnerService<S: HttpService>(S);

impl<S> hyper::service::Service for InnerService<S>
where
    S: HttpService + 'static,
{
    type ReqBody = hyper::Body;
    type ResBody = InnerBody<S>;
    type Error = BoxedStdError;
    type Future = InnerServiceFuture<S>;

    #[inline]
    fn call(&mut self, request: Request<hyper::Body>) -> Self::Future {
        let request = request.map(RequestBody::from_hyp);
        InnerServiceFuture {
            inner: self.0.respond(request),
        }
    }
}

#[allow(missing_debug_implementations)]
struct InnerServiceFuture<S: HttpService> {
    inner: S::Future,
}

impl<S> Future for InnerServiceFuture<S>
where
    S: HttpService,
{
    type Item = Response<InnerBody<S>>;
    type Error = BoxedStdError;

    #[inline]
    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        self.inner
            .poll()
            .map_async(|response| response.into_response().map(|inner| InnerBody { inner }))
            .map_err(Into::into)
    }
}

#[allow(missing_debug_implementations)]
struct InnerBody<S: HttpService> {
    inner: <S::Response as HttpResponseImpl>::Body,
}

impl<S> hyper::body::Payload for InnerBody<S>
where
    S: HttpService + 'static,
{
    type Data = <S::Response as HttpResponseImpl>::Data;
    type Error = BoxedStdError;

    #[inline]
    fn poll_data(&mut self) -> Poll<Option<Self::Data>, Self::Error> {
        ResponseBodyImpl::poll_data(&mut self.inner)
    }

    #[inline]
    fn poll_trailers(&mut self) -> Poll<Option<http::HeaderMap>, Self::Error> {
        ResponseBodyImpl::poll_trailers(&mut self.inner)
    }

    #[inline]
    fn content_length(&self) -> Option<u64> {
        let hint = ResponseBodyImpl::size_hint(&self.inner);
        match (hint.lower(), hint.upper()) {
            (lower, Some(upper)) if lower == upper => Some(upper),
            _ => None,
        }
    }
}

/// An implementation of `StreamService` consisting of a pair of an I/O stream
/// and `MakeService`.
#[derive(Debug)]
pub struct Incoming<S, I, T = NoTls> {
    make_service: S,
    incoming: I,
    tls: T,
}

impl<S, I, T> Incoming<S, I, T>
where
    S: MakeHttpService<I::Item, T::Transport>,
    I: Stream,
    I::Error: Into<BoxedStdError>,
    T: MakeTlsTransport<I::Item>,
{
    /// Create a new `Incoming` using the specified values.
    pub fn new(make_service: S, incoming: I, tls: T) -> Self {
        Self {
            make_service,
            incoming,
            tls,
        }
    }
}

impl<S, T> Incoming<S, crate::net::tcp::AddrIncoming, T>
where
    S: MakeHttpService<crate::net::tcp::AddrStream, T::Transport>,
    T: MakeTlsTransport<crate::net::tcp::AddrStream>,
{
    pub fn bind_tcp<A>(make_service: S, addr: A, tls: T) -> io::Result<Self>
    where
        A: std::net::ToSocketAddrs,
    {
        let incoming = crate::net::tcp::AddrIncoming::bind(addr)?;
        Ok(Incoming::new(make_service, incoming, tls))
    }

    #[doc(hidden)]
    pub fn local_addr(&self) -> std::net::SocketAddr {
        self.incoming.local_addr()
    }
}

#[cfg(unix)]
impl<S, T> Incoming<S, crate::net::unix::AddrIncoming, T>
where
    S: MakeHttpService<crate::net::unix::AddrStream, T::Transport>,
    T: MakeTlsTransport<crate::net::unix::AddrStream>,
{
    pub fn bind_unix<P>(make_service: S, path: P, tls: T) -> io::Result<Self>
    where
        P: AsRef<std::path::Path>,
    {
        let incoming = crate::net::unix::AddrIncoming::bind(path)?;
        Ok(Incoming::new(make_service, incoming, tls))
    }

    #[doc(hidden)]
    pub fn local_addr(&self) -> &std::os::unix::net::SocketAddr {
        self.incoming.local_addr()
    }
}

impl<S, I, T> StreamService for Incoming<S, I, T>
where
    S: MakeHttpService<I::Item, T::Transport>,
    I: Stream,
    I::Error: Into<BoxedStdError>,
    T: MakeTlsTransport<I::Item>,
{
    type Response = (S::Service, T::Transport);
    type Error = BoxedStdError;
    type Future = IncomingFuture<S::Future, T::Future>;

    fn poll_next_service(&mut self) -> Poll<Option<Self::Future>, Self::Error> {
        let stream = match futures::try_ready!(self.incoming.poll().map_err(Into::into)) {
            Some(stream) => stream,
            None => return Ok(Async::Ready(None)),
        };
        let make_service_future = self.make_service.make_service(&stream);
        let make_transport_future = self.tls.make_transport(stream);
        Ok(Async::Ready(Some(IncomingFuture {
            inner: (
                make_service_future.map_err(Into::into as fn(_) -> _),
                make_transport_future.map_err(Into::into as fn(_) -> _),
            )
                .into_future(),
        })))
    }
}

#[doc(hidden)]
#[allow(missing_debug_implementations, clippy::type_complexity)]
pub struct IncomingFuture<FutS, FutT>
where
    FutS: Future,
    FutS::Error: Into<BoxedStdError>,
    FutT: Future,
    FutT::Error: Into<BoxedStdError>,
{
    inner: futures::future::Join<
        futures::future::MapErr<FutS, fn(FutS::Error) -> BoxedStdError>,
        futures::future::MapErr<FutT, fn(FutT::Error) -> BoxedStdError>, //
    >,
}

impl<FutS, FutT> Future for IncomingFuture<FutS, FutT>
where
    FutS: Future,
    FutS::Item: IntoHttpService<FutT::Item>,
    FutS::Error: Into<BoxedStdError>,
    FutT: Future,
    FutT::Error: Into<BoxedStdError>,
{
    type Item = (
        <FutS::Item as IntoHttpService<FutT::Item>>::Service,
        FutT::Item,
    );
    type Error = BoxedStdError;

    #[inline]
    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let (into_service, transport) = futures::try_ready!(self.inner.poll());
        let service = into_service.into_service(&transport);
        Ok(Async::Ready((service, transport)))
    }
}
