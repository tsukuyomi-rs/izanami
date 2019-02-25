//! Abstraction around HTTP services.

use {
    crate::{
        drain::{Signal, Watch},
        request::RequestBody,
        BoxedStdError,
    },
    futures::{future::Executor, Async, Future, Poll},
    http::{Request, Response},
    hyper::server::conn::Http,
    izanami_http::{body::ContentLength, BodyTrailers, HttpBody, HttpService},
    izanami_rt::{Runnable, Runtime, Spawn, Spawner},
    izanami_service::Service,
    izanami_util::*,
    tokio::io::{AsyncRead, AsyncWrite},
    tokio_buf::BufStream,
};

/// A set of protocol level configuration.
#[derive(Debug)]
pub struct Protocol(Http);

impl Protocol {
    pub fn http1_only(&mut self, enabled: bool) -> &mut Self {
        self.0.http1_only(enabled);
        self
    }

    pub fn http1_half_close(&mut self, enabled: bool) -> &mut Self {
        self.0.http1_half_close(enabled);
        self
    }

    pub fn http1_writev(&mut self, enabled: bool) -> &mut Self {
        self.0.http1_writev(enabled);
        self
    }

    pub fn http2_only(&mut self, enabled: bool) -> &mut Self {
        self.0.http2_only(enabled);
        self
    }

    pub fn keep_alive(&mut self, enabled: bool) -> &mut Self {
        self.0.keep_alive(enabled);
        self
    }

    pub fn pipeline_flush(&mut self, enabled: bool) -> &mut Self {
        self.0.pipeline_flush(enabled);
        self
    }

    pub fn max_buf_size(&mut self, amt: usize) -> &mut Self {
        self.0.max_buf_size(amt);
        self
    }
}

/// A trait abstracting a factory that produces the connection with a client
/// and the service associated with its connection.
pub trait MakeConnection {
    /// A transport for sending/receiving data with the client.
    type Transport: AsyncRead + AsyncWrite;

    /// A `Service` that handles incoming requests from the client.
    type Service: HttpService<RequestBody>;

    /// The error type that will be thrown when acquiring a connection.
    type Error;

    ///　A `Future` that establishes the connection to client and initializes the service.
    type Future: Future<Item = (Self::Service, Self::Transport, Protocol), Error = Self::Error>;

    /// Polls the connection from client, and create a future that establishes
    /// its connection asynchronously.
    fn make_connection(&mut self, protocol: Protocol) -> Poll<Self::Future, Self::Error>;
}

impl<T, I, S> MakeConnection for T
where
    T: Service<Protocol, Response = (S, I, Protocol)>,
    S: HttpService<RequestBody>,
    I: AsyncRead + AsyncWrite,
{
    type Service = S;
    type Transport = I;
    type Error = T::Error;
    type Future = T::Future;

    fn make_connection(&mut self, protocol: Protocol) -> Poll<Self::Future, Self::Error> {
        futures::try_ready!(self.poll_ready());
        Ok(Async::Ready(self.call(protocol)))
    }
}

/// An HTTP server.
#[derive(Debug)]
pub struct Server<T, Sig = futures::future::Empty<(), ()>> {
    make_connection: T,
    shutdown_signal: Sig,
    protocol: Http,
}

impl<T> Server<T>
where
    T: MakeConnection,
{
    /// Creates a `Server` using a `MakeConnection`.
    pub fn new(make_connection: T) -> Self {
        Self {
            make_connection,
            shutdown_signal: futures::future::empty(),
            protocol: Http::new(),
        }
    }

    /// Specifies the signal to shutdown the background tasks gracefully.
    pub fn with_graceful_shutdown<Sig>(self, signal: Sig) -> Server<T, Sig>
    where
        Sig: Future<Item = ()>,
    {
        Server {
            make_connection: self.make_connection,
            shutdown_signal: signal,
            protocol: self.protocol,
        }
    }
}

impl<T, Sig> Server<T, Sig>
where
    T: MakeConnection,
    Sig: Future<Item = ()>,
{
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
            let executor = executor.clone();
            move |fut: <$S as MakeConnection>::Future, watch: Watch| {
                let executor = executor.clone();
                fut.map_err(|_e| log::error!("stream service error"))
                    .and_then(move |(service, stream, protocol)| {
                        let conn = protocol
                            .0
                            .with_executor(executor)
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
                make_connection: this.make_connection,
                shutdown_signal: this.shutdown_signal,
                protocol: this.protocol,
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
        impl<T, I, S, Sig> Spawn<$t> for Server<T, Sig>
        where
            T: MakeConnection<Transport = I, Service = S> + Send + 'static,
            T::Future: Send + 'static,
            I: AsyncRead + AsyncWrite + Send + 'static,
            S: HttpService<RequestBody> + Send + 'static,
            S::ResponseBody: Send + 'static,
            <S::ResponseBody as BufStream>::Item: Send,
            <S::ResponseBody as BufStream>::Error: Into<BoxedStdError>,
            <S::ResponseBody as BodyTrailers>::TrailersError: Into<BoxedStdError>,
            S::Error: Into<BoxedStdError>,
            S::Future: Send + 'static,
            Sig: Future<Item = ()> + Send + 'static,
        {
            fn spawn(self, $rt: &mut $t) {
                let $task = spawn_all_task!(<T> self, $rt.executor())
                    .map_err(|_e| log::error!("stream service error"));
                $e;
            }
        }
    };

    (!Send $t:ty) => {
        impl_spawn_for_server!(!Send $t, (rt, task) => { rt.spawn(task); });
    };
    (!Send $t:ty, ($rt:ident, $task:ident) => $e:expr) => {
        impl<T, I, S, Sig> Spawn<$t> for Server<T, Sig>
        where
            T: MakeConnection<Transport = I, Service = S> + 'static,
            T::Future: 'static,
            I: AsyncRead + AsyncWrite + Send + 'static,
            S: HttpService<RequestBody> + 'static,
            S::ResponseBody: Send + 'static,
            <S::ResponseBody as BufStream>::Item: Send,
            <S::ResponseBody as BufStream>::Error: Into<BoxedStdError>,
            <S::ResponseBody as BodyTrailers>::TrailersError: Into<BoxedStdError>,
            S::Error: Into<BoxedStdError>,
            Sig: Future<Item = ()> + 'static,
        {
            fn spawn(self, $rt: &mut $t) {
                let $task = spawn_all_task!(<T> self, $rt.executor())
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

impl<T, I, S, Sig> Runnable<tokio::runtime::Runtime> for Server<T, Sig>
where
    T: MakeConnection<Transport = I, Service = S> + Send + 'static,
    T::Error: Send + 'static,
    T::Future: Send + 'static,
    I: AsyncRead + AsyncWrite + Send + 'static,
    S: HttpService<RequestBody> + Send + 'static,
    S::ResponseBody: Send + 'static,
    <S::ResponseBody as BufStream>::Item: Send,
    <S::ResponseBody as BufStream>::Error: Into<BoxedStdError>,
    <S::ResponseBody as BodyTrailers>::TrailersError: Into<BoxedStdError>,
    S::Error: Into<BoxedStdError>,
    S::Future: Send + 'static,
    Sig: Future<Item = ()> + Send + 'static,
{
    type Output = Result<(), T::Error>;

    fn run(self, rt: &mut tokio::runtime::Runtime) -> Self::Output {
        let task = spawn_all_task!(<T> self, rt.executor());
        rt.block_on(task)
    }
}

impl<T, I, S, Sig> Runnable<tokio::runtime::current_thread::Runtime> for Server<T, Sig>
where
    T: MakeConnection<Transport = I, Service = S>,
    T::Future: 'static,
    I: AsyncRead + AsyncWrite + Send + 'static,
    S: HttpService<RequestBody> + 'static,
    S::ResponseBody: Send + 'static,
    <S::ResponseBody as BufStream>::Item: Send,
    <S::ResponseBody as BufStream>::Error: Into<BoxedStdError>,
    <S::ResponseBody as BodyTrailers>::TrailersError: Into<BoxedStdError>,
    S::Error: Into<BoxedStdError>,
    Sig: Future<Item = ()>,
{
    type Output = Result<(), T::Error>;

    fn run(self, rt: &mut tokio::runtime::current_thread::Runtime) -> Self::Output {
        let task = spawn_all_task!(<T> self, rt.executor());
        rt.block_on(task)
    }
}

struct SpawnAll<S, Sig, F, E> {
    state: SpawnAllState<S, Sig, F, E>,
}

enum SpawnAllState<T, Sig, F, E> {
    Running {
        make_connection: T,
        shutdown_signal: Sig,
        protocol: Http,
        serve_connection_fn: F,
        signal: Option<Signal>,
        watch: Watch,
        executor: E,
    },
    Done(crate::drain::Draining),
}

impl<T, I, S, Sig, F, Fut, E> Future for SpawnAll<T, Sig, F, E>
where
    T: MakeConnection<Transport = I, Service = S>,
    I: AsyncRead + AsyncWrite + Send + 'static,
    S: HttpService<RequestBody>,
    S::ResponseBody: Send + 'static,
    S::Error: Into<BoxedStdError>,
    Sig: Future<Item = ()>,
    F: FnMut(T::Future, Watch) -> Fut,
    Fut: Future<Item = (), Error = ()>,
    E: Executor<Fut>,
{
    type Item = ();
    type Error = T::Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        loop {
            self.state = match self.state {
                SpawnAllState::Running {
                    ref mut make_connection,
                    ref mut shutdown_signal,
                    ref mut serve_connection_fn,
                    ref mut signal,
                    ref protocol,
                    ref watch,
                    ref executor,
                } => match shutdown_signal.poll() {
                    Ok(Async::Ready(())) | Err(..) => {
                        let signal = signal.take().expect("unexpected condition");
                        SpawnAllState::Done(signal.drain())
                    }
                    Ok(Async::NotReady) => {
                        let fut = futures::try_ready!(
                            make_connection.make_connection(Protocol(protocol.clone()))
                        );
                        let serve_connection = serve_connection_fn(fut, watch.clone());
                        executor
                            .execute(serve_connection)
                            .unwrap_or_else(|_e| log::error!("executor error"));
                        continue;
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
struct InnerService<S>(S);

impl<S> hyper::service::Service for InnerService<S>
where
    S: HttpService<RequestBody> + 'static,
    S::ResponseBody: Send + 'static,
    <S::ResponseBody as BufStream>::Item: Send,
    <S::ResponseBody as BufStream>::Error: Into<BoxedStdError>,
    <S::ResponseBody as BodyTrailers>::TrailersError: Into<BoxedStdError>,
    S::Error: Into<BoxedStdError>,
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
struct InnerServiceFuture<S: HttpService<RequestBody>> {
    inner: S::Future,
}

impl<S> Future for InnerServiceFuture<S>
where
    S: HttpService<RequestBody>,
    S::ResponseBody: Send + 'static,
    <S::ResponseBody as BufStream>::Item: Send,
    <S::ResponseBody as BufStream>::Error: Into<BoxedStdError>,
    <S::ResponseBody as BodyTrailers>::TrailersError: Into<BoxedStdError>,
    S::Error: Into<BoxedStdError>,
{
    type Item = Response<InnerBody<S>>;
    type Error = BoxedStdError;

    #[inline]
    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        self.inner
            .poll()
            .map_async(|response| response.map(|inner| InnerBody { inner }))
            .map_err(Into::into)
    }
}

#[allow(missing_debug_implementations)]
struct InnerBody<S: HttpService<RequestBody>> {
    inner: S::ResponseBody,
}

impl<S> hyper::body::Payload for InnerBody<S>
where
    S: HttpService<RequestBody> + 'static,
    S::ResponseBody: Send + 'static,
    <S::ResponseBody as BufStream>::Item: Send,
    <S::ResponseBody as BufStream>::Error: Into<BoxedStdError>,
    <S::ResponseBody as BodyTrailers>::TrailersError: Into<BoxedStdError>,
{
    type Data = <S::ResponseBody as BufStream>::Item;
    type Error = BoxedStdError;

    #[inline]
    fn poll_data(&mut self) -> Poll<Option<Self::Data>, Self::Error> {
        BufStream::poll_buf(&mut self.inner).map_err(Into::into)
    }

    #[inline]
    fn poll_trailers(&mut self) -> Poll<Option<http::HeaderMap>, Self::Error> {
        BodyTrailers::poll_trailers(&mut self.inner).map_err(Into::into)
    }

    #[inline]
    fn content_length(&self) -> Option<u64> {
        match HttpBody::content_length(&self.inner) {
            ContentLength::Sized(len) => Some(len),
            ContentLength::Chunked => None,
        }
    }
}
