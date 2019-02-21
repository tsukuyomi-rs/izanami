//! Abstraction around HTTP services.

use {
    crate::{
        drain::{Signal, Watch},
        request::RequestBody,
        util::*,
        BoxedStdError,
    },
    futures::{future::Executor, Async, Future, Poll},
    http::{Request, Response},
    hyper::server::conn::Http,
    izanami_buf::BufStream,
    izanami_http::{body::ContentLength, BodyTrailers, HttpBody, HttpService},
    izanami_rt::{Runnable, Runtime, Spawn, Spawner},
    izanami_service::StreamService,
    tokio::io::{AsyncRead, AsyncWrite},
};

/// A builder for creating an `Server`.
#[derive(Debug)]
pub struct Builder<S, Sig = futures::future::Empty<(), ()>> {
    pub(crate) stream_service: S,
    pub(crate) protocol: Http,
    pub(crate) shutdown_signal: Sig,
}

impl<S, Sig> Builder<S, Sig>
where
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
    pub fn build<C, T>(self) -> Server<S, Sig>
    where
        S: StreamService<Response = (C, T)>,
        C: HttpService<RequestBody>,
        C::Error: Into<BoxedStdError>,
        T: AsyncRead + AsyncWrite,
    {
        Server {
            stream_service: self.stream_service,
            protocol: self.protocol,
            shutdown_signal: self.shutdown_signal,
        }
    }

    pub fn run<C, T>(self)
    where
        S: StreamService<Response = (C, T)>,
        C: HttpService<RequestBody>,
        C::Error: Into<BoxedStdError>,
        T: AsyncRead + AsyncWrite,
        Server<S, Sig>: Spawn<tokio::runtime::Runtime>,
    {
        izanami_rt::run(self.build())
    }
}

/// An HTTP server.
#[derive(Debug)]
pub struct Server<S, Sig = futures::future::Empty<(), ()>> {
    pub(crate) stream_service: S,
    pub(crate) protocol: Http,
    pub(crate) shutdown_signal: Sig,
}

impl<S> Server<S> {
    /// Creates a `Builder` using a streamed service.
    pub fn builder(stream_service: S) -> Builder<S> {
        Builder::new(stream_service, Http::new(), futures::future::empty())
    }
}

impl<S, C, T, Sig> Server<S, Sig>
where
    S: StreamService<Response = (C, T)>,
    C: HttpService<RequestBody>,
    C::Error: Into<BoxedStdError>,
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
            C: HttpService<RequestBody> + Send + 'static,
            C::ResponseBody: Send + 'static,
            <C::ResponseBody as BufStream>::Item: Send,
            <C::ResponseBody as BufStream>::Error: Into<BoxedStdError>,
            <C::ResponseBody as BodyTrailers>::TrailersError: Into<BoxedStdError>,
            C::Error: Into<BoxedStdError>,
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
            C: HttpService<RequestBody> + 'static,
            C::ResponseBody: Send + 'static,
            <C::ResponseBody as BufStream>::Item: Send,
            <C::ResponseBody as BufStream>::Error: Into<BoxedStdError>,
            <C::ResponseBody as BodyTrailers>::TrailersError: Into<BoxedStdError>,
            C::Error: Into<BoxedStdError>,
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
    C: HttpService<RequestBody> + Send + 'static,
    C::ResponseBody: Send + 'static,
    <C::ResponseBody as BufStream>::Item: Send,
    <C::ResponseBody as BufStream>::Error: Into<BoxedStdError>,
    <C::ResponseBody as BodyTrailers>::TrailersError: Into<BoxedStdError>,
    C::Error: Into<BoxedStdError>,
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
    C: HttpService<RequestBody> + 'static,
    C::ResponseBody: Send + 'static,
    <C::ResponseBody as BufStream>::Item: Send,
    <C::ResponseBody as BufStream>::Error: Into<BoxedStdError>,
    <C::ResponseBody as BodyTrailers>::TrailersError: Into<BoxedStdError>,
    C::Error: Into<BoxedStdError>,
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
    C: HttpService<RequestBody>,
    C::ResponseBody: Send + 'static,
    C::Error: Into<BoxedStdError>,
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
