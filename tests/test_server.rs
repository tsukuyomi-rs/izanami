#![allow(clippy::redundant_closure)]

use {
    http::{Request, Response},
    izanami_service::{MakeService, Service},
    std::io,
};

#[test]
fn version_sync() {
    version_sync::assert_html_root_url_updated!("src/lib.rs");
}

#[derive(Default)]
struct Echo(());

impl<Ctx, Bd> MakeService<Ctx, Request<Bd>> for Echo {
    type Response = Response<String>;
    type Error = io::Error;
    type Service = Self;
    type MakeError = io::Error;
    type Future = futures::future::FutureResult<Self::Service, Self::MakeError>;

    fn make_service(&self, _: Ctx) -> Self::Future {
        futures::future::ok(Echo::default())
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
        izanami::http::Http, //
        std::{
            io,
            net::{SocketAddr, TcpListener as StdTcpListener},
        },
        tokio::net::TcpStream,
    };

    #[test]
    fn tcp_server() -> izanami::Result<()> {
        izanami::system::current_thread(|sys| {
            let listener = StdTcpListener::bind("127.0.0.1:0")?;
            let local_addr = listener.local_addr()?;

            let http_server = Http::bind(listener) //
                .serve(super::Echo::default())?;

            let mut handle = sys.spawn(http_server);

            let client = Client::builder() //
                .build(TestConnect { local_addr });

            let response = sys.block_on(
                client.request(
                    Request::get("http://localhost/") //
                        .body(Body::empty())?,
                ),
            )?;
            assert_eq!(response.status(), 200);

            let body = sys.block_on(response.into_body().concat2())?;
            assert_eq!(body.into_bytes(), "hello");

            handle.shutdown();
            handle.wait_complete(sys)?;

            Ok(())
        })
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
        izanami::http::Http, //
        std::{io, path::PathBuf},
        tempfile::Builder,
        tokio::net::UnixStream,
    };

    #[test]
    fn unix_server() -> izanami::Result<()> {
        izanami::system::current_thread(|sys| {
            let sock_tempdir = Builder::new().prefix("izanami-tests").tempdir()?;
            let sock_path = sock_tempdir.path().join("connect.sock");

            let http_server = Http::bind(sock_path.clone()) //
                .serve(super::Echo::default())?;

            let mut handle = sys.spawn(http_server);

            let client = Client::builder() //
                .build(TestConnect {
                    sock_path: sock_path.clone(),
                });

            let response = sys.block_on(
                client.request(
                    Request::get("http://localhost/") //
                        .body(Body::empty())?,
                ),
            )?;
            assert_eq!(response.status(), 200);

            let body = sys.block_on(response.into_body().concat2())?;
            assert_eq!(body.into_bytes(), "hello");

            handle.shutdown();
            handle.wait_complete(sys)?;

            Ok(())
        })
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

#[cfg(feature = "native-tls")]
mod native_tls {
    use {
        ::native_tls::{Certificate, TlsConnector},
        futures::{Future, Stream},
        http::Request,
        hyper::{
            client::{
                connect::{Connect, Connected, Destination},
                Client,
            },
            Body,
        },
        izanami::{http::Http, tls::native_tls::NativeTls},
        std::{
            io,
            net::{SocketAddr, TcpListener},
        },
        tokio::net::TcpStream,
        tokio_tls::TlsStream,
    };

    #[test]
    fn tls_server() -> izanami::Result<()> {
        izanami::system::current_thread(|sys| {
            const IDENTITY: &[u8] = include_bytes!("../test/identity.pfx");
            const CERTIFICATE: &[u8] = include_bytes!("../test/server-crt.pem");

            let listener = TcpListener::bind("127.0.0.1:0")?;
            let local_addr = listener.local_addr()?;
            let native_tls = NativeTls::from_pkcs12(IDENTITY, "mypass")?;
            let http_server = Http::bind(listener) //
                .with_tls(native_tls)
                .serve(super::Echo::default())?;

            let mut handle = sys.spawn(http_server);

            let client = Client::builder() //
                .build(TestConnect {
                    local_addr,
                    connector: TlsConnector::builder()
                        .add_root_certificate(Certificate::from_pem(CERTIFICATE)?)
                        .build()?
                        .into(),
                });

            let response = sys.block_on(
                client.request(
                    Request::get("http://localhost/") //
                        .body(Body::empty())?,
                ),
            )?;
            assert_eq!(response.status(), 200);

            let body = sys.block_on(
                response
                    .into_body() //
                    .concat2(),
            )?;
            assert_eq!(body.into_bytes(), "hello");

            handle.shutdown();
            handle.wait_complete(sys)?;

            Ok(())
        })
    }

    struct TestConnect {
        local_addr: SocketAddr,
        connector: tokio_tls::TlsConnector,
    }

    impl Connect for TestConnect {
        type Transport = TlsStream<TcpStream>;
        type Error = io::Error;
        type Future = Box<
            dyn Future<Item = (Self::Transport, Connected), Error = Self::Error> + Send + 'static,
        >;

        fn connect(&self, _: Destination) -> Self::Future {
            let connector = self.connector.clone();
            Box::new(
                TcpStream::connect(&self.local_addr)
                    .and_then(move |stream| {
                        connector
                            .connect("localhost", stream)
                            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
                    }) //
                    .map(|stream| (stream, Connected::new())),
            )
        }
    }
}

#[cfg(feature = "openssl")]
mod openssl {
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
        izanami::{http::Http, tls::openssl::Ssl},
        openssl::{
            pkey::PKey,
            ssl::{SslConnector, SslMethod, SslVerifyMode},
            x509::X509,
        },
        std::{
            io,
            net::{SocketAddr, TcpListener},
        },
        tokio::net::TcpStream,
        tokio_openssl::{SslConnectorExt, SslStream},
    };

    const CERTIFICATE: &[u8] = include_bytes!("../test/server-crt.pem");
    const PRIVATE_KEY: &[u8] = include_bytes!("../test/server-key.pem");

    #[test]
    fn tls_server() -> izanami::Result<()> {
        izanami::system::current_thread(|sys| {
            let listener = TcpListener::bind("127.0.0.1:0")?;
            let local_addr = listener.local_addr()?;

            let cert = X509::from_pem(CERTIFICATE)?;
            let pkey = PKey::private_key_from_pem(PRIVATE_KEY)?;
            let ssl = Ssl::single_cert(cert, pkey);
            let http_server = Http::bind(listener) //
                .with_tls(ssl)
                .serve(super::Echo::default())?;

            let mut handle = sys.spawn(http_server);

            let client = Client::builder() //
                .build(TestConnect {
                    local_addr,
                    connector: {
                        let cert = X509::from_pem(CERTIFICATE)?;
                        let pkey = PKey::private_key_from_pem(PRIVATE_KEY)?;
                        let mut builder = SslConnector::builder(SslMethod::tls())?;
                        builder.set_verify(SslVerifyMode::NONE);
                        builder.set_certificate(&cert)?;
                        builder.set_private_key(&pkey)?;
                        builder.build()
                    },
                });

            let response = sys.block_on(
                client.request(
                    Request::get("http://localhost/") //
                        .body(Body::empty())?,
                ),
            )?;
            assert_eq!(response.status(), 200);

            let body = sys.block_on(
                response
                    .into_body() //
                    .concat2(),
            )?;
            assert_eq!(body.into_bytes(), "hello");

            handle.shutdown();
            handle.wait_complete(sys)?;

            Ok(())
        })
    }

    struct TestConnect {
        local_addr: SocketAddr,
        connector: SslConnector,
    }

    impl Connect for TestConnect {
        type Transport = SslStream<TcpStream>;
        type Error = io::Error;
        type Future = Box<
            dyn Future<Item = (Self::Transport, Connected), Error = Self::Error> + Send + 'static,
        >;

        fn connect(&self, _: Destination) -> Self::Future {
            let connector = self.connector.clone();
            Box::new(
                TcpStream::connect(&self.local_addr)
                    .and_then(move |stream| {
                        connector
                            .connect_async("localhost", stream)
                            .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))
                    }) //
                    .map(|stream| (stream, Connected::new())),
            )
        }
    }
}

#[cfg(feature = "rustls")]
mod rustls {
    use {
        ::native_tls::Certificate,
        futures::{Future, Stream},
        http::Request,
        hyper::{
            client::{
                connect::{Connect, Connected, Destination},
                Client,
            },
            Body,
        },
        izanami::{
            http::Http, //
            tls::rustls::Rustls,
        },
        std::{
            io,
            net::{SocketAddr, TcpListener},
        },
        tokio::net::TcpStream,
        tokio_tls::TlsStream,
    };

    #[test]
    fn tls_server() -> izanami::Result<()> {
        izanami::system::current_thread(|sys| {
            const CERTIFICATE: &[u8] = include_bytes!("../test/server-crt.pem");
            const PRIVATE_KEY: &[u8] = include_bytes!("../test/server-key.pem");

            let listener = TcpListener::bind("127.0.0.1:0")?;
            let local_addr = listener.local_addr()?;
            let rustls = Rustls::no_client_auth() //
                .single_cert(CERTIFICATE, PRIVATE_KEY)?;
            let http_server = Http::bind(listener) //
                .with_tls(rustls)
                .serve(super::Echo::default())?;

            let mut handle = sys.spawn(http_server);

            // FIXME: use rustls
            let client = Client::builder() //
                .build(TestConnect {
                    local_addr,
                    connector: ::native_tls::TlsConnector::builder()
                        .add_root_certificate(Certificate::from_pem(CERTIFICATE)?)
                        .build()?
                        .into(),
                });

            let response = sys.block_on(
                client.request(
                    Request::get("http://localhost/") //
                        .body(Body::empty())?,
                ),
            )?;
            assert_eq!(response.status(), 200);

            let body = sys.block_on(
                response
                    .into_body() //
                    .concat2(),
            )?;
            assert_eq!(body.into_bytes(), "hello");

            handle.shutdown();
            handle.wait_complete(sys)?;

            Ok(())
        })
    }

    // FIXME: use rustls
    struct TestConnect {
        local_addr: SocketAddr,
        connector: tokio_tls::TlsConnector,
    }

    impl Connect for TestConnect {
        type Transport = TlsStream<TcpStream>;
        type Error = io::Error;
        type Future = Box<
            dyn Future<Item = (Self::Transport, Connected), Error = Self::Error> + Send + 'static,
        >;

        fn connect(&self, _: Destination) -> Self::Future {
            let connector = self.connector.clone();
            Box::new(
                TcpStream::connect(&self.local_addr)
                    .and_then(move |stream| {
                        connector
                            .connect("localhost", stream)
                            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
                    }) //
                    .map(|stream| (stream, Connected::new())),
            )
        }
    }
}
