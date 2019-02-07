use {
    crate::{
        net::Listener,
        server::{Server, ServerConfig, SpawnServer},
        service::MakeHttpService,
    },
    bytes::Bytes,
    futures::{Future, Poll, Stream},
    http::{Request, Response},
    hyper::client::{
        connect::{Connect, Connected, Destination},
        Client,
    },
    izanami_util::RemoteAddr,
    std::{io, net::SocketAddr},
    tokio::{
        io::{AsyncRead, AsyncWrite},
        net::{TcpListener, TcpStream},
        runtime::Runtime,
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
    fn create() -> crate::Result<Self> {
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
    fn create() -> crate::Result<Self> {
        Self::create()
    }

    fn shutdown_on_idle(self) -> crate::Result<()> {
        crate::server::Runtime::shutdown_on_idle(self.inner)
    }
}

impl<T, S> SpawnServer<T, S> for TestRuntime
where
    T: Listener,
    S: MakeHttpService<T::Conn>,
    tokio::runtime::Runtime: SpawnServer<T, S>,
{
    fn spawn_server(&mut self, config: ServerConfig<S, T>) -> crate::Result<()> {
        self.inner.spawn_server(config)
    }
}

/// An HTTP server for testing HTTP services.
#[derive(Debug)]
pub struct TestServer {
    inner: Server<TestRuntime>,
    connector: TestConnector,
}

impl TestServer {
    /// Create a `TestServer` using the specified service factory.
    pub fn new<S>(make_service: S) -> crate::Result<Self>
    where
        S: MakeHttpService<TestStream> + Send + 'static,
        TestRuntime: SpawnServer<TestListener, S>,
    {
        let runtime = TestRuntime::create()?;
        let listener = TestListener::new()?;
        let connector = listener.connector();
        Ok(Self {
            inner: Server::bind(listener)?
                .runtime(runtime)
                .launch(make_service)?,
            connector,
        })
    }

    /// Create a `TestClient` for sending HTTP requests to the background server.
    pub fn client(&mut self) -> TestClient<'_> {
        TestClient {
            runtime: &mut *self.inner,
            client: Client::builder() //
                .build(self.connector.clone()),
        }
    }

    pub fn runtime_mut(&mut self) -> &mut TestRuntime {
        &mut *self.inner
    }

    /// Send a shutdown signal to the background server and await its completion.
    pub fn shutdown(self) -> crate::Result<()> {
        self.inner.shutdown()
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
        request: Request<impl Into<TestBody>>,
    ) -> crate::Result<Response<TestBody>> {
        self.runtime
            .block_on(self.client.request(request.map(|body| body.into().0)))
            .map(|response| response.map(TestBody))
            .map_err(Into::into)
    }
}

#[derive(Debug)]
pub struct TestListener {
    inner: TcpListener,
    local_addr: SocketAddr,
}

impl TestListener {
    fn new() -> crate::Result<Self> {
        let inner = TcpListener::bind(&"127.0.0.1:0".parse()?)?;
        let local_addr = inner.local_addr()?;
        Ok(Self { inner, local_addr })
    }

    fn connector(&self) -> TestConnector {
        TestConnector {
            addr: self.local_addr,
        }
    }
}

impl Listener for TestListener {
    type Conn = TestStream;

    #[inline]
    fn poll_incoming(&mut self) -> Poll<(Self::Conn, RemoteAddr), io::Error> {
        self.inner
            .poll_incoming()
            .map(|x| x.map(|(conn, addr)| (TestStream { inner: conn }, addr)))
    }
}

#[derive(Debug)]
pub struct TestStream {
    inner: <TcpListener as Listener>::Conn,
}

impl io::Read for TestStream {
    #[inline]
    fn read(&mut self, dst: &mut [u8]) -> io::Result<usize> {
        self.inner.read(dst)
    }
}

impl io::Write for TestStream {
    #[inline]
    fn write(&mut self, src: &[u8]) -> io::Result<usize> {
        self.inner.write(src)
    }

    #[inline]
    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

impl AsyncRead for TestStream {
    #[inline]
    unsafe fn prepare_uninitialized_buffer(&self, buf: &mut [u8]) -> bool {
        self.inner.prepare_uninitialized_buffer(buf)
    }
}

impl AsyncWrite for TestStream {
    #[inline]
    fn shutdown(&mut self) -> Poll<(), io::Error> {
        AsyncWrite::shutdown(&mut self.inner)
    }
}

#[derive(Debug, Clone)]
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

#[derive(Debug)]
pub struct TestBody(hyper::Body);

impl TestBody {
    pub fn concat(self) -> impl Future<Item = Bytes, Error = io::Error> {
        self.0
            .concat2()
            .map(|chunk| chunk.into_bytes())
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
    }
}

impl From<()> for TestBody {
    fn from(_: ()) -> Self {
        TestBody(hyper::Body::empty())
    }
}

macro_rules! impl_from_for_test_body {
    ($($t:ty,)*) => {$(
        impl From<$t> for TestBody {
            fn from(data: $t) -> Self {
                TestBody(hyper::Body::from(data))
            }
        }
    )*};
}

impl_from_for_test_body! {
    &'static str,
    &'static [u8],
    String,
    Vec<u8>,
    Bytes,
}

impl Stream for TestBody {
    type Item = Bytes;
    type Error = io::Error;

    #[inline]
    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        self.0
            .poll()
            .map(|x| x.map(|opt| opt.map(|chunk| chunk.into_bytes())))
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
    }
}
