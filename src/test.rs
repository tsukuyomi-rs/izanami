use {
    crate::{
        io::Listener,
        server::{Serve, Server, ServerConfig, SpawnServer},
        service::MakeHttpService,
    },
    futures::Future,
    http::{Request, Response},
    hyper::client::{
        connect::{Connect, Connected, Destination},
        Client,
    },
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
pub struct TestRuntime {
    inner: Runtime,
}

impl std::ops::Deref for TestRuntime {
    type Target = Runtime;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl std::ops::DerefMut for TestRuntime {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

impl TestRuntime {
    fn new() -> crate::Result<Self> {
        let mut builder = tokio::runtime::Builder::new();
        builder.name_prefix("izanami");
        builder.core_threads(1);
        builder.blocking_threads(1);

        Ok(Self {
            inner: builder.build()?,
        })
    }
}

impl crate::server::Runtime for TestRuntime {
    fn new() -> crate::Result<Self> {
        Self::new()
    }

    fn shutdown(self) -> crate::Result<()> {
        self.inner.shutdown()
    }
}

impl<T, S, Sig> SpawnServer<T, S, Sig> for TestRuntime
where
    T: Listener,
    S: MakeHttpService<T>,
    Sig: Future<Item = ()>,
    tokio::runtime::Runtime: SpawnServer<T, S, Sig>,
{
    fn spawn_server(&mut self, config: ServerConfig<S, T, Sig>) -> crate::Result<()> {
        self.inner.spawn_server(config)
    }
}

/// An HTTP server for testing HTTP services.
#[derive(Debug)]
pub struct TestServer {
    serve: Serve<TestRuntime>,
    shutdown_signal: oneshot::Sender<()>,
    local_addr: SocketAddr,
}

impl TestServer {
    /// Create a `TestServer` using the specified service factory.
    pub fn new<S>(make_service: S) -> crate::Result<Self>
    where
        S: MakeHttpService<TcpListener> + Send + 'static,
        TestRuntime: SpawnServer<TcpListener, S, oneshot::Receiver<()>>,
    {
        let listener = TcpListener::bind(&"127.0.0.1:0".parse()?)?;
        let local_addr = listener.local_addr()?;

        let (tx, rx) = oneshot::channel();
        let serve = Server::bind(listener)?
            .with_graceful_shutdown(rx)
            .runtime(TestRuntime::new()?)
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
            runtime: self.serve.get_mut(),
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
    runtime: &'a mut TestRuntime,
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
