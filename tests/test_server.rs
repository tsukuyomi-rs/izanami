use {
    futures::Future,
    http::{Request, Response},
    izanami::Server,
    izanami_service::{MakeService, Service},
    std::{io, net::SocketAddr},
    tokio::net::TcpStream,
};

#[test]
fn version_sync() {
    version_sync::assert_html_root_url_updated!("src/lib.rs");
}

struct Echo;

impl<Ctx, Bd> MakeService<Ctx, Request<Bd>> for Echo {
    type Response = Response<String>;
    type Error = io::Error;
    type Service = Self;
    type MakeError = io::Error;
    type Future = futures::future::FutureResult<Self::Service, Self::MakeError>;

    fn make_service(&self, _: Ctx) -> Self::Future {
        futures::future::ok(Echo)
    }
}

impl<Bd> Service<Request<Bd>> for Echo {
    type Response = Response<String>;
    type Error = io::Error;
    type Future = futures::future::FutureResult<Self::Response, Self::Error>;

    fn poll_ready(&mut self) -> futures::Poll<(), Self::Error> {
        Ok(().into())
    }

    fn call(&mut self, _: Request<Bd>) -> Self::Future {
        futures::future::ok(Response::builder().body("hello".into()).unwrap())
    }
}

#[test]
fn smoketest_tcp() -> izanami::Result<()> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    let local_addr = listener.local_addr()?;

    let (tx, rx) = futures::sync::oneshot::channel();
    let handle = std::thread::spawn(move || -> izanami::Result<()> {
        let listener =
            tokio::net::TcpListener::from_std(listener, &tokio::reactor::Handle::default())?;
        let server = Server::bind(listener)?;
        server.start_with_graceful_shutdown(Echo, rx)?;
        Ok(())
    });

    {
        let mut runtime = tokio::runtime::current_thread::Runtime::new()?;
        let response = runtime
            .block_on(
                hyper::Client::builder()
                    .build(TestConnector { addr: local_addr })
                    .request(
                        Request::get("http://localhost/") //
                            .body(hyper::Body::empty())
                            .unwrap(),
                    ),
            )
            .expect("client error");
        assert_eq!(response.status(), 200);
    }

    // finally, send the shutdown signal to the backend HTTP server
    // and await its completion.
    tx.send(()).expect("failed to send shutdown signal");
    match handle.join() {
        Ok(result) => result,
        Err(err) => std::panic::resume_unwind(err),
    }
}

struct TestConnector {
    addr: SocketAddr,
}

impl hyper::client::connect::Connect for TestConnector {
    type Transport = TcpStream;
    type Error = io::Error;
    type Future = Box<
        dyn Future<
                Item = (Self::Transport, hyper::client::connect::Connected),
                Error = Self::Error,
            > + Send
            + 'static,
    >;

    fn connect(&self, _: hyper::client::connect::Destination) -> Self::Future {
        Box::new(
            TcpStream::connect(&self.addr)
                .map(|stream| (stream, hyper::client::connect::Connected::new())),
        )
    }
}
