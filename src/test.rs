use {
    crate::{
        server::{
            conn::Listener,
            imp::{BackendImpl, ServerConfig},
            Backend, MakeServiceContext, RequestBody, Serve, Server,
        },
        CritError,
    },
    futures::Future,
    http::{Request, Response},
    hyper::client::{
        connect::{Connect, Connected, Destination},
        Client,
    },
    izanami_service::{MakeService, Service},
    izanami_util::buf_stream::BufStream,
    std::{io, net::SocketAddr},
    tokio::{
        net::{TcpListener, TcpStream},
        runtime::Runtime,
        sync::oneshot,
    },
};

/// The implementor of `Backend` used in `TestServer`.
///
/// This backend launches the HTTP server using `tokio::runtime::Runtime`
/// customized for testing purposes.
#[derive(Debug)]
pub struct TestBackend {
    runtime: tokio::runtime::Runtime,
}

impl<T, S, Bd, SvcErr, Svc, MkErr, Fut, Sig> Backend<T, S, Sig> for TestBackend
where
    T: Listener + 'static,
    T::Conn: Send + 'static,
    T::Incoming: Send + 'static,
    S: for<'a> MakeService<
            MakeServiceContext<'a, T>, //
            Request<RequestBody>,
            Response = Response<Bd>,
            Error = SvcErr,
            Service = Svc,
            MakeError = MkErr,
            Future = Fut,
        > + Send
        + Sync
        + 'static,
    SvcErr: Into<CritError>,
    MkErr: Into<CritError>,
    Fut: Future<Item = Svc, Error = MkErr> + Send + 'static,
    Svc: Service<
            Request<RequestBody>, //
            Response = Response<Bd>,
            Error = SvcErr,
        > + Send
        + 'static,
    Svc::Future: Send + 'static,
    Bd: BufStream + Send + 'static,
    Bd::Item: Send,
    Bd::Error: Into<CritError>,
    Sig: Future<Item = ()> + Send + 'static,
{
}

impl<T, S, Bd, SvcErr, Svc, MkErr, Fut, Sig> BackendImpl<T, S, Sig> for TestBackend
where
    T: Listener + 'static,
    T::Conn: Send + 'static,
    T::Incoming: Send + 'static,
    S: for<'a> MakeService<
            MakeServiceContext<'a, T>, //
            Request<RequestBody>,
            Response = Response<Bd>,
            Error = SvcErr,
            Service = Svc,
            MakeError = MkErr,
            Future = Fut,
        > + Send
        + Sync
        + 'static,
    SvcErr: Into<CritError>,
    MkErr: Into<CritError>,
    Fut: Future<Item = Svc, Error = MkErr> + Send + 'static,
    Svc: Service<
            Request<RequestBody>, //
            Response = Response<Bd>,
            Error = SvcErr,
        > + Send
        + 'static,
    Svc::Future: Send + 'static,
    Bd: BufStream + Send + 'static,
    Bd::Item: Send,
    Bd::Error: Into<CritError>,
    Sig: Future<Item = ()> + Send + 'static,
{
    fn new() -> crate::Result<Self> {
        Ok(Self {
            runtime: {
                let mut builder = tokio::runtime::Builder::new();
                builder.name_prefix("izanami");
                builder.core_threads(1);
                builder.blocking_threads(1);
                builder.build()?
            },
        })
    }

    fn spawn(&mut self, config: ServerConfig<S, T, Sig>) -> crate::Result<()> {
        let incoming = config.listener.incoming();
        let protocol = config
            .protocol
            .with_executor(tokio::executor::DefaultExecutor::current());
        let serve = hyper::server::Builder::new(incoming, protocol) //
            .serve(config.make_service);

        if let Some(shutdown_signal) = config.shutdown_signal {
            self.runtime.spawn(
                serve
                    .with_graceful_shutdown(shutdown_signal)
                    .map_err(|e| log::error!("server error: {}", e)),
            );
        } else {
            self.runtime.spawn(
                serve //
                    .map_err(|e| log::error!("server error: {}", e)),
            );
        }

        Ok(())
    }

    fn shutdown(self) -> crate::Result<()> {
        self.runtime.shutdown_on_idle().wait().unwrap();
        Ok(())
    }
}

/// An HTTP server for testing HTTP services.
#[derive(Debug)]
pub struct TestServer<S> {
    serve: Serve<TestBackend, S, TcpListener, oneshot::Receiver<()>>,
    shutdown_signal: oneshot::Sender<()>,
    local_addr: SocketAddr,
}

impl<S> TestServer<S>
where
    S: for<'a> MakeService<MakeServiceContext<'a, TcpListener>, Request<RequestBody>>
        + Send
        + 'static,
    TestBackend: Backend<TcpListener, S, oneshot::Receiver<()>>,
{
    /// Create a `TestServer` using the specified service factory.
    pub fn new(make_service: S) -> crate::Result<Self> {
        let listener = TcpListener::bind(&"127.0.0.1:0".parse()?)?;
        let local_addr = listener.local_addr()?;

        let (tx, rx) = oneshot::channel();
        let serve = Server::bind(listener)?
            .backend::<TestBackend>()
            .with_graceful_shutdown(rx)
            .launch(make_service)?;

        Ok(Self {
            serve,
            shutdown_signal: tx,
            local_addr,
        })
    }

    /// Create a `TestClient` for sending HTTP requests to the background server.
    pub fn client(&mut self) -> TestClient<'_> {
        TestClient {
            runtime: &mut self.serve.get_mut().runtime,
            client: Client::builder() //
                .build(TestConnector {
                    addr: self.local_addr,
                }),
        }
    }

    /// Send a shutdown signal to the background server and await its completion.
    pub fn shutdown(self) -> crate::Result<()> {
        self.shutdown_signal
            .send(())
            .map_err(|_| failure::format_err!("failed to send shutdown signal"))?;
        self.serve.shutdown()
    }
}

/// An HTTP client used for sending requests to the background server.
#[derive(Debug)]
pub struct TestClient<'a> {
    client: Client<TestConnector, hyper::Body>,
    runtime: &'a mut Runtime,
}

impl<'a> TestClient<'a> {
    pub fn request(
        &mut self,
        request: Request<hyper::Body>,
    ) -> crate::Result<Response<hyper::Body>> {
        self.runtime
            .block_on(self.client.request(request))
            .map_err(Into::into)
    }
}

#[allow(missing_debug_implementations)]
struct TestConnector {
    addr: SocketAddr,
}

impl Connect for TestConnector {
    type Transport = TcpStream;
    type Error = io::Error;
    type Future =
        Box<dyn Future<Item = (Self::Transport, Connected), Error = Self::Error> + Send + 'static>;

    fn connect(&self, _: Destination) -> Self::Future {
        Box::new(TcpStream::connect(&self.addr).map(|stream| (stream, Connected::new())))
    }
}
