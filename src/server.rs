//! The implementation of HTTP server.

use {
    crate::{
        drain::{Signal, Watch},
        error::BoxedStdError,
        runtime::{Runtime, Spawn},
        service::{
            imp::{HttpResponseImpl, ResponseBodyImpl},
            HttpService, MakeHttpService, RequestBody,
        },
        tls::{MakeTlsTransport, NoTls},
        util::*,
    },
    futures::{future::Executor, Async, Future, IntoFuture, Poll, Stream},
    http::{Request, Response},
    hyper::server::conn::Http,
    izanami_service::StreamService,
    std::io,
    tokio::io::{AsyncRead, AsyncWrite},
};

/// A builder for creating an `Server`.
#[derive(Debug)]
pub struct Builder<S> {
    stream_service: S,
    protocol: Http,
}

impl<S, T, C> Builder<S>
where
    S: StreamService<Response = (T, C)>,
    T: AsyncRead + AsyncWrite,
    C: HttpService,
{
    /// Creates a `Builder` using a streamed service.
    pub fn new(stream_service: S, protocol: Http) -> Self {
        Self {
            stream_service,
            protocol,
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

    /// Consumes itself and create an instance of `Server`.
    ///
    /// It also returns a `Handle` used by controlling the spawned server.
    pub fn build(self) -> (Server<S>, Handle) {
        let (signal, watch) = crate::drain::channel();
        let task = Server {
            stream_service: self.stream_service,
            watch,
            protocol: self.protocol,
        };
        let handle = Handle { signal };
        (task, handle)
    }

    /// Starts the configured HTTP server onto the specified spawner and returns its handle.
    pub fn start<Spawner: ?Sized>(self, spawner: &mut Spawner) -> Handle
    where
        Server<S>: Spawn<Spawner>,
    {
        let (server, handle) = self.build();
        server.spawn(spawner);
        handle
    }
}

/// A handle associated with `Server`.
#[derive(Debug)]
pub struct Handle {
    signal: Signal,
}

impl Handle {
    /// Send a shutdown signal to the background task and await its completion.
    pub fn shutdown(self) -> impl Future<Item = (), Error = ()> {
        self.signal.drain()
    }
}

impl Future for Handle {
    type Item = ();
    type Error = ();

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        self.signal.poll()
    }
}

/// An HTTP server.
#[derive(Debug)]
pub struct Server<S> {
    stream_service: S,
    watch: Watch,
    protocol: Http,
}

impl<S, T, C> Server<S>
where
    S: StreamService<Response = (T, C)>,
    T: AsyncRead + AsyncWrite,
    C: HttpService,
{
    /// Creates a `Builder` using a streamed service.
    pub fn builder(stream_service: S) -> Builder<S> {
        Builder::new(stream_service, Http::new())
    }

    /// Spawns this server onto the specified spawner.
    pub fn spawn<Sp: ?Sized>(self, spawner: &mut Sp)
    where
        Self: Spawn<Sp>,
    {
        Spawn::spawn(self, spawner)
    }
}

impl<S, T> Server<Incoming<crate::net::tcp::AddrIncoming, S, T>>
where
    S: MakeHttpService<crate::net::tcp::AddrStream>,
    T: MakeTlsTransport<crate::net::tcp::AddrStream>,
{
    pub fn bind<A>(
        make_service: S,
        addr: A,
        tls: T,
    ) -> io::Result<Builder<Incoming<crate::net::tcp::AddrIncoming, S, T>>>
    where
        A: std::net::ToSocketAddrs,
    {
        let incoming = crate::net::tcp::AddrIncoming::bind(addr)?;
        Ok(Server::builder(Incoming::new(incoming, make_service, tls)))
    }

    #[doc(hidden)]
    pub fn local_addr(&self) -> std::net::SocketAddr {
        self.stream_service.incoming.local_addr()
    }
}

#[cfg(unix)]
impl<S, T> Server<Incoming<crate::net::unix::AddrIncoming, S, T>>
where
    S: MakeHttpService<crate::net::unix::AddrStream>,
    T: MakeTlsTransport<crate::net::unix::AddrStream>,
{
    pub fn bind_unix<P>(
        make_service: S,
        path: P,
        tls: T,
    ) -> io::Result<Builder<Incoming<crate::net::unix::AddrIncoming, S, T>>>
    where
        P: AsRef<std::path::Path>,
    {
        let incoming = crate::net::unix::AddrIncoming::bind(path)?;
        Ok(Server::builder(Incoming::new(incoming, make_service, tls)))
    }

    #[doc(hidden)]
    pub fn local_addr(&self) -> &std::os::unix::net::SocketAddr {
        self.stream_service.incoming.local_addr()
    }
}

/// A helper macro for creating a server task.
macro_rules! spawn_all_task {
    (<$S:ty> $self:expr, $executor:expr) => {{
        let this = $self;
        let executor = $executor;

        let watch = this.watch;
        let stream_service = this.stream_service;
        let protocol = this.protocol.with_executor(executor.clone());
        let serve_connection_fn = move |fut: <$S as StreamService>::Future, watch: Watch| {
            let protocol = protocol.clone();
            fut.map_err(|_e| log::error!("stream service error"))
                .and_then(move |(stream, service)| {
                    let service = InnerService(service);
                    let conn = protocol.serve_connection(stream, service).with_upgrades();
                    watch
                        .watching(conn, |c, _| c.poll(), |c| c.graceful_shutdown())
                        .map_err(|e| log::error!("connection error: {}", e))
                })
        };

        let spawn_all = SpawnAll {
            stream_service,
            executor,
            serve_connection_fn,
            state: SpawnAllState::Running,
        };

        watch.watching(
            spawn_all,
            |s, watch| s.poll_watching(watch),
            |s| s.shutdown(),
        )
    }};
}

impl<S, T, C> Spawn<tokio::runtime::Runtime> for Server<S>
where
    S: StreamService<Response = (T, C)> + Send + 'static,
    S::Future: Send + 'static,
    T: AsyncRead + AsyncWrite + Send + 'static,
    C: HttpService + Send + 'static,
    C::Future: Send + 'static,
{
    fn spawn(self, rt: &mut tokio::runtime::Runtime) {
        let task = spawn_all_task!(<S> self, rt.executor());
        rt.spawn(task);
    }
}

impl<S, T, C> Spawn<tokio::executor::DefaultExecutor> for Server<S>
where
    S: StreamService<Response = (T, C)> + Send + 'static,
    S::Future: Send + 'static,
    T: AsyncRead + AsyncWrite + Send + 'static,
    C: HttpService + Send + 'static,
    C::Future: Send + 'static,
{
    fn spawn(self, spawner: &mut tokio::executor::DefaultExecutor) {
        use tokio::executor::Executor;
        let task = spawn_all_task!(<S> self, spawner.clone());
        spawner
            .spawn(Box::new(task))
            .expect("failed to spawn a task");
    }
}

impl<S, T, C> Spawn<tokio::runtime::TaskExecutor> for Server<S>
where
    S: StreamService<Response = (T, C)> + Send + 'static,
    S::Future: Send + 'static,
    T: AsyncRead + AsyncWrite + Send + 'static,
    C: HttpService + Send + 'static,
    C::Future: Send + 'static,
{
    fn spawn(self, spawner: &mut tokio::runtime::TaskExecutor) {
        let task = spawn_all_task!(<S> self, spawner.clone());
        spawner.spawn(task);
    }
}

impl<S, T, C> Spawn<tokio::runtime::current_thread::Runtime> for Server<S>
where
    S: StreamService<Response = (T, C)> + 'static,
    S::Future: 'static,
    T: AsyncRead + AsyncWrite + Send + 'static,
    C: HttpService + 'static,
{
    fn spawn(self, rt: &mut tokio::runtime::current_thread::Runtime) {
        let task = spawn_all_task!(<S> self, rt.executor());
        rt.spawn(task);
    }
}

impl<S, T, C> Spawn<tokio::runtime::current_thread::TaskExecutor> for Server<S>
where
    S: StreamService<Response = (T, C)> + 'static,
    S::Future: 'static,
    T: AsyncRead + AsyncWrite + Send + 'static,
    C: HttpService + 'static,
{
    fn spawn(self, spawner: &mut tokio::runtime::current_thread::TaskExecutor) {
        let task = spawn_all_task!(<S> self, spawner.clone());
        spawner
            .spawn_local(Box::new(task))
            .expect("failed to spawn a task");
    }
}

#[allow(unused)]
struct SpawnAll<S, F, E> {
    stream_service: S,
    serve_connection_fn: F,
    executor: E,
    state: SpawnAllState,
}

enum SpawnAllState {
    Running,
    Done,
}

impl<S, F, E> SpawnAll<S, F, E> {
    fn shutdown(&mut self) {
        self.state = SpawnAllState::Done;
    }
}

impl<S, T, C, F, Fut, E> SpawnAll<S, F, E>
where
    S: StreamService<Response = (T, C)>,
    T: AsyncRead + AsyncWrite + Send + 'static,
    C: HttpService,
    F: FnMut(S::Future, Watch) -> Fut,
    Fut: Future<Item = (), Error = ()>,
    E: Executor<Fut>,
{
    fn poll_watching(&mut self, watch: &Watch) -> Poll<(), ()> {
        loop {
            match self.state {
                SpawnAllState::Running => match self.stream_service.poll_next_service() {
                    Ok(Async::Ready(Some(fut))) => {
                        let serve_connection = (self.serve_connection_fn)(fut, watch.clone());
                        self.executor
                            .execute(serve_connection)
                            .unwrap_or_else(|_e| log::error!("executor error"));
                    }
                    Ok(Async::Ready(None)) => {
                        self.state = SpawnAllState::Done;
                        return Ok(Async::Ready(()));
                    }
                    Ok(Async::NotReady) => return Ok(Async::NotReady),
                    Err(_err) => {
                        log::error!("stream service error");
                        self.state = SpawnAllState::Done;
                        return Ok(Async::Ready(()));
                    }
                },
                SpawnAllState::Done => return Ok(Async::Ready(())),
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

#[derive(Debug)]
pub struct Incoming<I, S, T = NoTls> {
    incoming: I,
    make_service: S,
    tls: T,
}

impl<I, S, T> Incoming<I, S, T>
where
    I: Stream,
    I::Error: Into<BoxedStdError>,
    S: MakeHttpService<I::Item>,
    T: MakeTlsTransport<I::Item>,
    T::Transport: AsyncRead + AsyncWrite,
{
    pub(crate) fn new(incoming: I, make_service: S, tls: T) -> Self {
        Self {
            incoming,
            make_service,
            tls,
        }
    }
}

impl<I, S, T> StreamService for Incoming<I, S, T>
where
    I: Stream,
    I::Error: Into<BoxedStdError>,
    S: MakeHttpService<I::Item>,
    T: MakeTlsTransport<I::Item>,
{
    type Response = (T::Transport, S::Service);
    type Error = BoxedStdError;
    type Future = IncomingFuture<T::Future, S::Future>;

    fn poll_next_service(&mut self) -> Poll<Option<Self::Future>, Self::Error> {
        let stream = match futures::try_ready!(self.incoming.poll().map_err(Into::into)) {
            Some(stream) => stream,
            None => return Ok(Async::Ready(None)),
        };
        let make_service_future = self.make_service.make_service(&stream);
        let make_transport_future = self.tls.make_transport(stream);
        Ok(Async::Ready(Some(IncomingFuture {
            inner: (
                make_transport_future.map_err(Into::into as fn(_) -> _),
                make_service_future.map_err(Into::into as fn(_) -> _),
            )
                .into_future(),
        })))
    }
}

#[doc(hidden)]
#[allow(missing_debug_implementations, clippy::type_complexity)]
pub struct IncomingFuture<FutT, FutS>
where
    FutT: Future,
    FutT::Error: Into<BoxedStdError>,
    FutS: Future,
    FutS::Error: Into<BoxedStdError>,
{
    inner: futures::future::Join<
        futures::future::MapErr<FutT, fn(FutT::Error) -> BoxedStdError>, //
        futures::future::MapErr<FutS, fn(FutS::Error) -> BoxedStdError>,
    >,
}

impl<FutT, FutS> Future for IncomingFuture<FutT, FutS>
where
    FutT: Future,
    FutT::Error: Into<BoxedStdError>,
    FutS: Future,
    FutS::Error: Into<BoxedStdError>,
{
    type Item = (FutT::Item, FutS::Item);
    type Error = BoxedStdError;

    #[inline]
    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        self.inner.poll()
    }
}
