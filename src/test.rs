use {
    crate::{
        server::{
            conn::Listener,
            imp::{BackendImpl, LiftedMakeHttpService},
            Backend, MakeServiceContext, RequestBody,
        },
        CritError,
    },
    futures::sync::oneshot::{self, Receiver, Sender},
    futures::Future,
    http::{Request, Response},
    hyper::{
        client::{
            connect::{Connect, Connected, Destination},
            Client,
        },
        server::conn::Http,
    },
    izanami_service::{MakeService, Service},
    izanami_util::buf_stream::BufStream,
    std::{
        io,
        net::SocketAddr,
        thread::{self, JoinHandle},
    },
    tokio::{
        net::{TcpListener, TcpStream},
        runtime::current_thread::Runtime,
    },
};

/// The implementor of `Backend` used in `TestServer`.
///
/// This backend launches the HTTP server using `tokio::runtime::Runtime`
/// customized for testing purposes.
#[derive(Debug)]
pub struct TestBackend(());

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
    fn start(
        protocol: Http,
        incoming: T::Incoming,
        make_service: S,
        shutdown_signal: Option<Sig>,
    ) -> crate::Result<()> {
        let protocol = protocol.with_executor(tokio::executor::DefaultExecutor::current());
        let serve = hyper::server::Builder::new(incoming, protocol) //
            .serve(LiftedMakeHttpService::new(make_service));

        let mut runtime = {
            let mut builder = tokio::runtime::Builder::new();
            builder.name_prefix("izanami");
            builder.core_threads(1);
            builder.blocking_threads(1);
            builder.build()?
        };

        if let Some(shutdown_signal) = shutdown_signal {
            runtime.spawn(
                serve
                    .with_graceful_shutdown(shutdown_signal)
                    .map_err(|e| log::error!("server error: {}", e)),
            );
        } else {
            runtime.spawn(
                serve //
                    .map_err(|e| log::error!("server error: {}", e)),
            );
        }

        runtime.shutdown_on_idle().wait().unwrap();

        Ok(())
    }
}

/// An HTTP server for testing HTTP services.
#[derive(Debug)]
pub struct TestServer {
    join_handle: JoinHandle<crate::Result<()>>,
    shutdown_signal: Sender<()>,
    local_addr: SocketAddr,
    runtime: Runtime,
}

impl TestServer {
    /// Create a `TestServer` using the specified service factory.
    pub fn new<S>(make_service: S) -> crate::Result<Self>
    where
        S: for<'a> MakeService<MakeServiceContext<'a, TcpListener>, Request<RequestBody>>
            + Send
            + 'static,
        TestBackend: Backend<TcpListener, S, Receiver<()>>,
    {
        let listener = TcpListener::bind(&"127.0.0.1:0".parse()?)?;
        let local_addr = listener.local_addr()?;

        let (tx, rx) = oneshot::channel();
        let join_handle = thread::spawn(move || -> crate::Result<()> {
            crate::Server::bind(listener)?
                .backend::<TestBackend>()
                .start_with_graceful_shutdown(make_service, rx)?;
            Ok(())
        });

        Ok(Self {
            join_handle,
            shutdown_signal: tx,
            local_addr,
            runtime: Runtime::new()?,
        })
    }

    /// Create a `TestClient` for sending HTTP requests to the background server.
    pub fn client(&mut self) -> TestClient<'_> {
        TestClient {
            runtime: &mut self.runtime,
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
        match self.join_handle.join() {
            Ok(result) => result,
            Err(err) => std::panic::resume_unwind(err),
        }
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
