use {
    bytes::Bytes,
    futures::{Future, Stream},
    http::{Request, Response},
    hyper::{
        client::connect::{Connect, Connected, Destination},
        Body, Client,
    },
    izanami_server::{
        net::tcp::{AddrIncoming, AddrStream},
        Connection, Server,
    },
    izanami_service::ServiceExt,
    std::{io, net::SocketAddr},
    tokio::{net::TcpStream, runtime::current_thread::Runtime, sync::oneshot},
};

struct TestConnect {
    local_addr: SocketAddr,
}

impl Connect for TestConnect {
    type Transport = TcpStream;
    type Error = io::Error;
    type Future =
        Box<dyn Future<Item = (Self::Transport, Connected), Error = Self::Error> + Send + 'static>;

    fn connect(&self, _: Destination) -> Self::Future {
        Box::new(
            TcpStream::connect(&self.local_addr) //
                .map(|stream| (stream, Connected::new())),
        )
    }
}

pub(crate) struct TestServer {
    inner: Client<TestConnect, Body>,
    runtime: Runtime,
    tx_shutdown: oneshot::Sender<()>,
}

impl TestServer {
    pub(crate) fn start_h1<T>(
        dispatch: impl Fn(AddrStream) -> T + Clone + 'static,
    ) -> io::Result<Self>
    where
        T: Connection + 'static,
    {
        Self::start(dispatch, false)
    }

    pub(crate) fn start_h2<T>(
        dispatch: impl Fn(AddrStream) -> T + Clone + 'static,
    ) -> io::Result<Self>
    where
        T: Connection + 'static,
    {
        Self::start(dispatch, true)
    }

    fn start<T>(
        dispatch: impl Fn(AddrStream) -> T + Clone + 'static,
        h2_only: bool,
    ) -> io::Result<Self>
    where
        T: Connection + 'static,
    {
        let mut runtime = Runtime::new()?;

        let incoming = AddrIncoming::bind("127.0.0.1:0")?;
        let local_addr = incoming.local_addr();

        let inner = {
            let mut client_builder = Client::builder();
            if h2_only {
                client_builder.http2_only(true);
            }
            client_builder.build(TestConnect { local_addr })
        };

        let (tx_shutdown, rx_shutdown) = oneshot::channel();
        let server = Server::builder(incoming.service_map(dispatch))
            .with_graceful_shutdown(rx_shutdown)
            .spawner(tokio::runtime::current_thread::TaskExecutor::current())
            .build()
            .map_err(|_e| eprintln!("server error"));
        runtime.spawn(Box::new(server));

        Ok(Self {
            inner,
            runtime,
            tx_shutdown,
        })
    }

    pub(crate) fn respond(&mut self, req: Request<Body>) -> hyper::Result<Response<Bytes>> {
        let (parts, body) = self.runtime.block_on(self.inner.request(req))?.into_parts();
        let body = self.runtime.block_on(body.concat2())?;
        Ok(Response::from_parts(parts, body.into_bytes()))
    }

    pub(crate) fn shutdown(mut self) {
        let _ = self.tx_shutdown.send(());
        drop(self.inner);
        self.runtime.run().unwrap();
    }
}
