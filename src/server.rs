use {
    crate::{
        io::{AcceptWith, Listener, MakeListener},
        service::{Context, HttpService, MakeHttpService, RequestBody, ResponseBody},
        tls::Acceptor,
    },
    futures::{Future, Poll},
    http::{Request, Response},
    hyper::server::conn::Http,
    izanami_util::RemoteAddr,
    std::marker::PhantomData,
};

/// A simple HTTP server that wraps the `hyper`'s server implementation.
#[derive(Debug)]
pub struct Server<
    T, //
    Sig = futures::future::Empty<(), ()>,
    Rt = tokio::runtime::Runtime,
> {
    listener: T,
    protocol: Http,
    shutdown_signal: Option<Sig>,
    runtime: Option<Rt>,
}

impl Server<()> {
    /// Creates an HTTP server from the specified listener.
    pub fn bind<T>(listener: T) -> Result<Server<T::Listener>, T::Error>
    where
        T: MakeListener,
    {
        Ok(Server {
            listener: listener.make_listener()?,
            protocol: Http::new(),
            shutdown_signal: None,
            runtime: None,
        })
    }
}

impl<T, Sig, Rt> Server<T, Sig, Rt>
where
    T: Listener,
    Sig: Future<Item = ()>,
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
    pub fn accept<A>(self, acceptor: A) -> Server<AcceptWith<T, A>, Sig, Rt>
    where
        A: Acceptor<T::Conn>,
    {
        Server {
            listener: self.listener.accept_with(acceptor),
            protocol: self.protocol,
            shutdown_signal: self.shutdown_signal,
            runtime: self.runtime,
        }
    }

    pub fn runtime<Rt2>(self, runtime: Rt2) -> Server<T, Sig, Rt2>
    where
        Rt2: Runtime,
    {
        Server {
            listener: self.listener,
            protocol: self.protocol,
            shutdown_signal: self.shutdown_signal,
            runtime: Some(runtime),
        }
    }

    /// Switches the backend to `CurrentThread`.
    pub fn current_thread(self) -> Server<T, Sig, tokio::runtime::current_thread::Runtime> {
        Server {
            listener: self.listener,
            protocol: self.protocol,
            shutdown_signal: self.shutdown_signal,
            runtime: None,
        }
    }

    /// Set the value of shutdown signal.
    pub fn with_graceful_shutdown<Sig2>(self, signal: Sig2) -> Server<T, Sig2, Rt>
    where
        Sig2: Future<Item = ()>,
    {
        Server {
            listener: self.listener,
            protocol: self.protocol,
            shutdown_signal: Some(signal),
            runtime: self.runtime,
        }
    }

    pub fn launch<S>(self, make_service: S) -> crate::Result<Serve<Rt>>
    where
        S: MakeHttpService<T>,
        Rt: Runtime + SpawnServer<T, S, Sig>,
    {
        let mut runtime = match self.runtime {
            Some(rt) => rt,
            None => Rt::new()?,
        };

        runtime.spawn_server(ServerConfig {
            make_service: MakeIzanamiService {
                make_service,
                _marker: PhantomData,
            },
            listener: self.listener,
            protocol: self.protocol,
            shutdown_signal: self.shutdown_signal,
        })?;

        Ok(Serve { runtime })
    }

    /// Start this HTTP server with the specified service factory.
    pub fn start<S>(self, make_service: S) -> crate::Result<()>
    where
        S: MakeHttpService<T>,
        Rt: Runtime + SpawnServer<T, S, Sig>,
    {
        self.launch(make_service)?.shutdown()
    }
}

#[derive(Debug)]
pub struct Serve<Rt: Runtime = tokio::runtime::Runtime> {
    runtime: Rt,
}

impl<Rt> Serve<Rt>
where
    Rt: Runtime,
{
    pub fn get_ref(&self) -> &Rt {
        &self.runtime
    }

    pub fn get_mut(&mut self) -> &mut Rt {
        &mut self.runtime
    }

    pub fn shutdown(self) -> crate::Result<()> {
        self.runtime.shutdown()
    }
}

pub trait Runtime {
    fn new() -> crate::Result<Self>
    where
        Self: Sized;

    fn shutdown(self) -> crate::Result<()>;
}

impl Runtime for tokio::runtime::Runtime {
    fn new() -> crate::Result<Self> {
        Ok(Self::new()?)
    }

    fn shutdown(self) -> crate::Result<()> {
        self.shutdown_on_idle().wait().unwrap();
        Ok(())
    }
}

impl Runtime for tokio::runtime::current_thread::Runtime {
    fn new() -> crate::Result<Self> {
        Ok(Self::new()?)
    }

    fn shutdown(mut self) -> crate::Result<()> {
        self.run().unwrap();
        Ok(())
    }
}

#[doc(hidden)]
#[allow(missing_debug_implementations)]
pub struct ServerConfig<S, T, Sig> {
    make_service: MakeIzanamiService<T, S>,
    listener: T,
    protocol: Http,
    shutdown_signal: Option<Sig>,
}

/// A trait for abstracting the process around executing the HTTP server.
pub trait SpawnServer<T, S, Sig = futures::future::Empty<(), ()>> {
    fn spawn_server(&mut self, config: ServerConfig<S, T, Sig>) -> crate::Result<()>;
}

impl<T, S, Sig> SpawnServer<T, S, Sig> for tokio::runtime::Runtime
where
    T: Listener + 'static,
    T::Conn: Send + 'static,
    T::Incoming: Send + 'static,
    S: MakeHttpService<T> + Send + Sync + 'static,
    S::Future: Send + 'static,
    S::Service: Send + 'static,
    <S::Service as HttpService>::Future: Send + 'static,
    Sig: Future<Item = ()> + Send + 'static,
{
    fn spawn_server(&mut self, config: ServerConfig<S, T, Sig>) -> crate::Result<()> {
        let incoming = config.listener.incoming();
        let protocol = config.protocol.with_executor(self.executor());

        let serve = hyper::server::Builder::new(incoming, protocol) //
            .serve(config.make_service);
        if let Some(shutdown_signal) = config.shutdown_signal {
            self.spawn(
                serve
                    .with_graceful_shutdown(shutdown_signal)
                    .map_err(|e| log::error!("server error: {}", e)),
            );
        } else {
            self.spawn(
                serve //
                    .map_err(|e| log::error!("server error: {}", e)),
            );
        }

        Ok(())
    }
}

impl<T, S, Sig> SpawnServer<T, S, Sig> for tokio::runtime::current_thread::Runtime
where
    T: Listener + 'static,
    T::Conn: Send + 'static,
    T::Incoming: 'static,
    S: MakeHttpService<T> + 'static,
    S::Service: 'static,
    S::Future: 'static,
    Sig: Future<Item = ()> + 'static,
{
    fn spawn_server(&mut self, config: ServerConfig<S, T, Sig>) -> crate::Result<()> {
        let incoming = config.listener.incoming();
        let protocol = config
            .protocol
            .with_executor(tokio::runtime::current_thread::TaskExecutor::current());

        let serve = hyper::server::Builder::new(incoming, protocol) //
            .serve(config.make_service);

        if let Some(shutdown_signal) = config.shutdown_signal {
            self.spawn(
                serve
                    .with_graceful_shutdown(shutdown_signal)
                    .map_err(|e| log::error!("server error: {}", e)),
            );
        } else {
            self.spawn(
                serve //
                    .map_err(|e| log::error!("server error: {}", e)),
            );
        }

        Ok(())
    }
}

#[allow(missing_debug_implementations)]
struct MakeIzanamiService<T, S> {
    make_service: S,
    _marker: PhantomData<fn(&T)>,
}

impl<'a, T, S> hyper::service::MakeService<&'a T::Conn> for MakeIzanamiService<T, S>
where
    T: Listener,
    S: MakeHttpService<T>,
{
    type ReqBody = hyper::Body;
    type ResBody = WrappedBodyStream<S::ResponseBody>;
    type Error = S::Error;
    type Service = IzanamiService<S::Service>;
    type MakeError = S::MakeError;
    type Future = MakeIzanamiServiceFuture<S::Future>;

    fn make_service(&mut self, conn: &'a T::Conn) -> Self::Future {
        MakeIzanamiServiceFuture {
            inner: self.make_service.make_service(Context::new(conn)),
            remote_addr: Some(T::remote_addr(conn)),
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
