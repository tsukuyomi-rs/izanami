#[test]
fn version_sync() {
    version_sync::assert_html_root_url_updated!("src/lib.rs");
}

mod tcp {
    use {
        futures::{Future, Stream},
        http::{Request, Response},
        hyper::{
            client::{
                connect::{Connect, Connected, Destination},
                Client,
            },
            Body,
        },
        izanami::{
            h1::H1, //
            h2::H2,
            net::tcp::AddrIncoming,
            server::Server,
            service::ext::ServiceExt,
        },
        std::{io, net::SocketAddr},
        tokio::{
            net::TcpStream, //
            runtime::current_thread::Runtime,
            sync::oneshot,
        },
    };

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

    #[test]
    fn tcp_server() -> failure::Fallible<()> {
        let mut rt = Runtime::new()?;

        let incoming = AddrIncoming::bind("127.0.0.1:0")?;
        let local_addr = incoming.local_addr();
        let make_connection = incoming //
            .service_map(|stream| {
                H1::new().serve(
                    stream,
                    izanami::service::service_fn(|_req| {
                        Response::builder()
                            .header("content-type", "text/plain")
                            .body("hello")
                    }),
                )
            });

        let (tx_shutdown, rx_shutdown) = oneshot::channel();
        let server = Server::builder(make_connection)
            .with_graceful_shutdown(rx_shutdown)
            .build()
            .map_err(|e| eprintln!("server error: {}", e));
        rt.spawn(Box::new(server));

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

    #[test]
    fn h2_server() -> failure::Fallible<()> {
        let mut rt = Runtime::new()?;

        let incoming = AddrIncoming::bind("127.0.0.1:0")?;
        let local_addr = incoming.local_addr();
        let make_connection = incoming //
            .service_map(|stream| {
                H2::new().serve(
                    stream,
                    izanami::service::service_fn(|_req| {
                        Response::builder()
                            .header("content-type", "text/plain")
                            .body("hello")
                    }),
                )
            });

        let (tx_shutdown, rx_shutdown) = oneshot::channel();
        let server = Server::builder(make_connection)
            .with_graceful_shutdown(rx_shutdown)
            .build()
            .map_err(|e| eprintln!("server error: {}", e));
        rt.spawn(Box::new(server));

        let client = Client::builder() //
            .http2_only(true)
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
}

#[cfg(unix)]
mod unix {
    use {
        futures::{Future, Stream},
        http::{Request, Response},
        hyper::{
            client::{
                connect::{Connect, Connected, Destination},
                Client,
            },
            Body,
        },
        izanami::{h1::H1, net::unix::AddrIncoming, server::Server, service::ext::ServiceExt},
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

        let (tx_shutdown, rx_shutdown) = oneshot::channel();
        let server = Server::builder(
            AddrIncoming::bind(&sock_path)? //
                .service_map(|stream| {
                    H1::new().serve(
                        stream,
                        izanami::service::service_fn(|_req| {
                            Response::builder()
                                .header("content-type", "text/plain")
                                .body("hello")
                        }),
                    )
                }),
        )
        .with_graceful_shutdown(rx_shutdown)
        .build()
        .map_err(|e| eprintln!("server error: {}", e));
        rt.spawn(Box::new(server));

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
