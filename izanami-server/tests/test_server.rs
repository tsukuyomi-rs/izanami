#[test]
fn version_sync() {
    version_sync::assert_html_root_url_updated!("src/lib.rs");
}

mod tcp {
    use {
        futures::{Future, Stream},
        http::Request,
        hyper::{
            client::{
                connect::{Connect, Connected, Destination},
                Client,
            },
            Body,
        },
        izanami_server::{incoming::Incoming, Server},
        std::{io, net::SocketAddr},
        tokio::{
            net::TcpStream, //
            runtime::current_thread::Runtime,
            sync::oneshot,
        },
    };

    #[test]
    fn tcp_server() -> failure::Fallible<()> {
        let mut rt = Runtime::new()?;

        let incoming = izanami_net::tcp::AddrIncoming::bind("127.0.0.1:0")?;
        let local_addr = incoming.local_addr();

        let stream_service = Incoming::builder(incoming) //
            .serve(izanami_service::service_fn_ok(|()| {
                izanami_service::service_fn_ok(|_req| {
                    http::Response::builder()
                        .header("content-type", "text/plain")
                        .body("hello")
                        .unwrap()
                })
            }));

        let (tx_shutdown, rx_shutdown) = oneshot::channel();
        let server = Server::new(stream_service) //
            .with_graceful_shutdown(rx_shutdown);
        server.start(&mut rt);

        let client = Client::builder() //
            .build(TestConnect { local_addr });

        let response = rt.block_on(
            client.request(
                Request::get("http://localhost/") //
                    .body(Body::empty())?,
            ),
        )?;
        assert_eq!(response.status(), 200);

        let body = rt.block_on(response.into_body().concat2())?;
        assert_eq!(body.into_bytes(), "hello");

        drop(client);
        let _ = tx_shutdown.send(());
        rt.run().unwrap();
        Ok(())
    }

    struct TestConnect {
        local_addr: SocketAddr,
    }

    impl Connect for TestConnect {
        type Transport = TcpStream;
        type Error = io::Error;
        type Future = Box<
            dyn Future<Item = (Self::Transport, Connected), Error = Self::Error> + Send + 'static,
        >;

        fn connect(&self, _: Destination) -> Self::Future {
            Box::new(
                TcpStream::connect(&self.local_addr) //
                    .map(|stream| (stream, Connected::new())),
            )
        }
    }
}

#[cfg(unix)]
mod unix {
    use {
        futures::{Future, Stream},
        http::Request,
        hyper::{
            client::{
                connect::{Connect, Connected, Destination},
                Client,
            },
            Body,
        },
        izanami_net::unix::AddrIncoming,
        izanami_server::{incoming::Incoming, Server},
        std::{io, path::PathBuf},
        tempfile::Builder,
        tokio::{
            net::UnixStream, //
            runtime::current_thread::Runtime,
            sync::oneshot,
        },
    };

    #[test]
    fn unix_server() -> failure::Fallible<()> {
        let mut rt = Runtime::new()?;

        let sock_tempdir = Builder::new().prefix("izanami-tests").tempdir()?;
        let sock_path = sock_tempdir.path().join("connect.sock");

        let incoming = AddrIncoming::bind(&sock_path)?;
        let stream_service = Incoming::builder(incoming) //
            .serve(izanami_service::service_fn_ok(|()| {
                izanami_service::service_fn_ok(|_req| {
                    http::Response::builder()
                        .header("content-type", "text/plain")
                        .body("hello")
                        .unwrap()
                })
            }));

        let (tx_shutdown, rx_shutdown) = oneshot::channel();
        let server = Server::new(stream_service) //
            .with_graceful_shutdown(rx_shutdown);
        server.start(&mut rt);

        let client = Client::builder() //
            .build(TestConnect {
                sock_path: sock_path.clone(),
            });

        let response = rt.block_on(
            client.request(
                Request::get("http://localhost/") //
                    .body(Body::empty())?,
            ),
        )?;
        assert_eq!(response.status(), 200);

        let body = rt.block_on(response.into_body().concat2())?;
        assert_eq!(body.into_bytes(), "hello");

        drop(client);
        let _ = tx_shutdown.send(());
        rt.run().unwrap();
        Ok(())
    }

    struct TestConnect {
        sock_path: PathBuf,
    }

    impl Connect for TestConnect {
        type Transport = UnixStream;
        type Error = io::Error;
        type Future = Box<
            dyn Future<Item = (Self::Transport, Connected), Error = Self::Error> + Send + 'static,
        >;

        fn connect(&self, _: Destination) -> Self::Future {
            Box::new(
                UnixStream::connect(&self.sock_path) //
                    .map(|stream| (stream, Connected::new())),
            )
        }
    }
}
