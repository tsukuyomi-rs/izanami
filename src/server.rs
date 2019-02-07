use {
    crate::{
        net::{AcceptWith, Bind, Listener},
        service::{
            Context, //
            HttpService,
            MakeHttpService,
            RequestBody,
            ResponseBody,
        },
        tls::Acceptor,
    },
    futures::{Future, Poll, Stream},
    http::{Request, Response},
    hyper::server::conn::Http,
    izanami_util::RemoteAddr,
    std::{io, marker::PhantomData},
    tokio::{
        io::{AsyncRead, AsyncWrite},
        sync::oneshot,
    },
};

/// A simple HTTP server that wraps the `hyper`'s server implementation.
#[derive(Debug)]
pub struct Server<
    T, //
    Rt = tokio::runtime::Runtime,
> {
    listener: T,
    protocol: Http,
    runtime: Option<Rt>,
}

impl Server<()> {
    /// Creates an HTTP server from the specified listener.
    pub fn bind<B>(bind: B) -> crate::Result<Server<B::Listener>>
    where
        B: Bind,
    {
        Ok(Server {
            listener: bind.bind()?,
            protocol: Http::new(),
            runtime: None,
        })
    }
}

impl<T, Rt> Server<T, Rt>
where
    T: Listener,
    Rt: Runtime,
{
    /// Returns a reference to the inner listener.
    pub fn listener(&self) -> &T {
        &self.listener
    }

    /// Returns a mutable reference to the inner listener.
    pub fn listener_mut(&mut self) -> &mut T {
        &mut self.listener
    }

    /// Returns a reference to the HTTP-level configuration.
    pub fn protocol(&self) -> &Http {
        &self.protocol
    }

    /// Returns a mutable reference to the HTTP-level configuration.
    pub fn protocol_mut(&mut self) -> &mut Http {
        &mut self.protocol
    }

    /// Sets the instance of `Acceptor` to the server.
    pub fn accept<A>(self, acceptor: A) -> Server<AcceptWith<T, A>, Rt>
    where
        A: Acceptor<T::Conn>,
    {
        Server {
            listener: AcceptWith::new(self.listener, acceptor),
            protocol: self.protocol,
            runtime: self.runtime,
        }
    }

    /// Specify the instance of runtime to use spawning the server task.
    pub fn runtime<Rt2>(self, runtime: Rt2) -> Server<T, Rt2>
    where
        Rt2: Runtime,
    {
        Server {
            listener: self.listener,
            protocol: self.protocol,
            runtime: Some(runtime),
        }
    }

    /// Start an HTTP server using the specified service factory.
    pub fn launch<S>(self, make_service: S) -> crate::Result<Serve<Rt>>
    where
        S: MakeHttpService<T::Conn>,
        Rt: SpawnServer<T, S>,
    {
        let mut runtime = match self.runtime {
            Some(rt) => rt,
            None => Rt::create()?,
        };

        let (tx, rx) = oneshot::channel();

        runtime.spawn_server(ServerConfig {
            make_service,
            listener: self.listener,
            protocol: self.protocol,
            shutdown_signal: rx,
        })?;

        Ok(Serve {
            runtime,
            shutdown_signal: tx,
        })
    }

    /// Start an HTTP server using the specified service factory.
    pub fn start<S>(self, make_service: S) -> crate::Result<()>
    where
        S: MakeHttpService<T::Conn>,
        Rt: SpawnServer<T, S>,
    {
        self.launch(make_service)?.run()
    }
}

#[derive(Debug)]
pub struct Serve<Rt: Runtime = tokio::runtime::Runtime> {
    runtime: Rt,
    shutdown_signal: oneshot::Sender<()>,
}

impl<Rt> Serve<Rt>
where
    Rt: Runtime,
{
    /// Wait for completion of all tasks executing on the runtime
    /// without sending shutdown signal.
    pub fn run(self) -> crate::Result<()> {
        self.runtime.shutdown_on_idle()
    }

    /// Send a shutdown signal to the server task and wait for
    /// completion of all tasks executing on the runtime.
    pub fn shutdown(self) -> crate::Result<()> {
        self.shutdown_signal
            .send(())
            .expect("failed to send shutdown signal");
        self.runtime.shutdown_on_idle()?;
        Ok(())
    }
}

impl<Rt> std::ops::Deref for Serve<Rt>
where
    Rt: Runtime,
{
    type Target = Rt;

    fn deref(&self) -> &Self::Target {
        &self.runtime
    }
}

impl<Rt> std::ops::DerefMut for Serve<Rt>
where
    Rt: Runtime,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.runtime
    }
}

/// A trait abstracting the runtime that executes asynchronous tasks.
pub trait Runtime {
    /// Create an instance of this runtime.
    ///
    /// This function is called by `Server` when the instance
    /// of runtime is not given explicitly.
    fn create() -> crate::Result<Self>
    where
        Self: Sized;

    /// Wait for completion of background tasks executing on
    /// this runtime.
    fn shutdown_on_idle(self) -> crate::Result<()>;
}

impl Runtime for tokio::runtime::Runtime {
    fn create() -> crate::Result<Self> {
        Ok(Self::new()?)
    }

    fn shutdown_on_idle(self) -> crate::Result<()> {
        self.shutdown_on_idle().wait().unwrap();
        Ok(())
    }
}

impl Runtime for tokio::runtime::current_thread::Runtime {
    fn create() -> crate::Result<Self> {
        Ok(Self::new()?)
    }

    fn shutdown_on_idle(mut self) -> crate::Result<()> {
        self.run().unwrap();
        Ok(())
    }
}

#[doc(hidden)]
#[allow(missing_debug_implementations)]
pub struct ServerConfig<S, T> {
    make_service: S,
    listener: T,
    protocol: Http,
    shutdown_signal: oneshot::Receiver<()>,
}

/// A trait that represents spawning of HTTP server tasks.
pub trait SpawnServer<T, S>
where
    T: Listener,
    S: MakeHttpService<T::Conn>,
{
    fn spawn_server(&mut self, config: ServerConfig<S, T>) -> crate::Result<()>;
}

/// A helper macro for creating a server task.
macro_rules! serve {
    (<$T:ty, $S:ty> $config:expr, $executor:expr) => {{
        let config = $config;
        let executor = $executor;

        let incoming = AddrIncoming {
            listener: config.listener,
        };

        let protocol = config.protocol.with_executor(executor);

        hyper::server::Builder::new(incoming, protocol) //
            .serve(MakeIzanamiService::<$T, $S>::new(config.make_service))
            .with_graceful_shutdown(config.shutdown_signal)
            .map_err(|e| log::error!("server error: {}", e))
    }};
}

impl<T, S> SpawnServer<T, S> for tokio::runtime::Runtime
where
    T: Listener + Send + 'static,
    T::Conn: Send + 'static,
    S: MakeHttpService<T::Conn> + Send + Sync + 'static,
    S::Future: Send + 'static,
    S::Service: Send + 'static,
    <S::Service as HttpService>::Future: Send + 'static,
{
    fn spawn_server(&mut self, config: ServerConfig<S, T>) -> crate::Result<()> {
        self.spawn(serve!(<T, S> config, self.executor()));
        Ok(())
    }
}

impl<T, S> SpawnServer<T, S> for tokio::runtime::current_thread::Runtime
where
    T: Listener + Send + 'static,
    T::Conn: Send + 'static,
    S: MakeHttpService<T::Conn> + 'static,
    S::Service: 'static,
    S::Future: 'static,
{
    fn spawn_server(&mut self, config: ServerConfig<S, T>) -> crate::Result<()> {
        self.spawn(serve!(<T, S> config, tokio::runtime::current_thread::TaskExecutor::current()));
        Ok(())
    }
}

#[allow(missing_debug_implementations)]
struct MakeIzanamiService<T, S> {
    make_service: S,
    _marker: PhantomData<fn(&T)>,
}

impl<T, S> MakeIzanamiService<T, S>
where
    T: Listener,
    S: MakeHttpService<T::Conn>,
{
    fn new(make_service: S) -> Self {
        Self {
            make_service,
            _marker: PhantomData,
        }
    }
}

impl<'a, T, S> hyper::service::MakeService<&'a AddrStream<T::Conn>> for MakeIzanamiService<T, S>
where
    T: Listener,
    S: MakeHttpService<T::Conn>,
{
    type ReqBody = hyper::Body;
    type ResBody = WrappedBodyStream<S::ResponseBody>;
    type Error = S::Error;
    type Service = IzanamiService<S::Service>;
    type MakeError = S::MakeError;
    type Future = MakeIzanamiServiceFuture<S::Future>;

    fn make_service(&mut self, conn: &'a AddrStream<T::Conn>) -> Self::Future {
        MakeIzanamiServiceFuture {
            inner: self.make_service.make_service(Context::new(&conn.io)),
            remote_addr: Some(conn.remote_addr.clone()),
        }
    }
}

#[allow(missing_debug_implementations)]
struct MakeIzanamiServiceFuture<Fut> {
    inner: Fut,
    remote_addr: Option<RemoteAddr>,
}

impl<Fut> Future for MakeIzanamiServiceFuture<Fut>
where
    Fut: Future,
{
    type Item = IzanamiService<Fut::Item>;
    type Error = Fut::Error;

    #[inline]
    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        Ok(IzanamiService {
            service: futures::try_ready!(self.inner.poll()),
            remote_addr: self
                .remote_addr
                .take()
                .expect("the future has already been polled."),
        }
        .into())
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
    type ResBody = WrappedBodyStream<S::ResponseBody>;
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
    type Item = Response<WrappedBodyStream<Bd>>;
    type Error = Fut::Error;

    #[inline]
    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        self.0
            .poll()
            .map(|x| x.map(|response| response.map(WrappedBodyStream)))
    }
}

#[allow(missing_debug_implementations)]
struct WrappedBodyStream<Bd>(Bd);

impl<Bd> hyper::body::Payload for WrappedBodyStream<Bd>
where
    Bd: ResponseBody + Send + 'static,
{
    type Data = Bd::Item;
    type Error = Bd::Error;

    fn poll_data(&mut self) -> Poll<Option<Self::Data>, Self::Error> {
        self.0.poll_buf()
    }

    fn content_length(&self) -> Option<u64> {
        self.0.size_hint().upper()
    }
}

#[allow(missing_debug_implementations)]
struct AddrIncoming<T: Listener> {
    listener: T,
}

impl<T> Stream for AddrIncoming<T>
where
    T: Listener,
{
    type Item = AddrStream<T::Conn>;
    type Error = io::Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        self.listener
            .poll_incoming()
            .map(|x| x.map(|(io, remote_addr)| Some(AddrStream { io, remote_addr })))
    }
}

#[allow(missing_debug_implementations)]
struct AddrStream<T> {
    io: T,
    remote_addr: RemoteAddr,
}

impl<T> io::Read for AddrStream<T>
where
    T: AsyncRead + AsyncWrite,
{
    #[inline]
    fn read(&mut self, dst: &mut [u8]) -> io::Result<usize> {
        self.io.read(dst)
    }
}

impl<T> io::Write for AddrStream<T>
where
    T: AsyncRead + AsyncWrite,
{
    #[inline]
    fn write(&mut self, src: &[u8]) -> io::Result<usize> {
        self.io.write(src)
    }

    #[inline]
    fn flush(&mut self) -> io::Result<()> {
        self.io.flush()
    }
}

impl<T> AsyncRead for AddrStream<T>
where
    T: AsyncRead + AsyncWrite,
{
    #[inline]
    unsafe fn prepare_uninitialized_buffer(&self, buf: &mut [u8]) -> bool {
        self.io.prepare_uninitialized_buffer(buf)
    }
}

impl<T> AsyncWrite for AddrStream<T>
where
    T: AsyncRead + AsyncWrite,
{
    #[inline]
    fn shutdown(&mut self) -> Poll<(), io::Error> {
        self.io.shutdown()
    }
}
