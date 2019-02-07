use {
    http::{Request, Response},
    izanami::test::TestServer,
    izanami_service::{MakeService, Service},
    std::io,
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
fn test_server() -> izanami::Result<()> {
    let mut server = TestServer::new(Echo)?;

    let response = server
        .client() //
        .request(
            Request::get("http://localhost/") //
                .body(())?,
        )?;
    assert_eq!(response.status(), 200);

    let body = server
        .runtime_mut()
        .block_on(response.into_body().concat())?;
    assert_eq!(body, "hello");

    Ok(())
}

#[cfg(unix)]
mod uds {
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
        izanami::Server, //
        std::{io, path::PathBuf},
        tempfile::Builder,
        tokio::{
            net::UnixStream, //
            runtime::current_thread::Runtime,
        },
    };

    #[test]
    fn uds_server() -> izanami::Result<()> {
        let dir = Builder::new().prefix("izanami-tests").tempdir().unwrap();
        let sock_path = dir.path().join("connect.sock");

        let runtime = Runtime::new()?;
        let mut serve = Server::bind(&*sock_path)?
            .runtime(runtime)
            .launch(super::Echo)?;

        let client = Client::builder() //
            .build(TestConnect {
                sock_path: sock_path.clone(),
            });

        let response = serve //
            .block_on(
                client.request(
                    Request::get("http://localhost/") //
                        .body(Body::empty())?,
                ),
            )?;
        assert_eq!(response.status(), 200);

        let body = serve.block_on(response.into_body().concat2())?;
        assert_eq!(body.into_bytes(), "hello");

        drop(client);
        serve.shutdown()
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
        izanami::{tls::native_tls::TlsAcceptor, Server},
        std::{io, net::SocketAddr},
        tokio::{
            net::{TcpListener, TcpStream}, //
            runtime::current_thread::Runtime,
        },
        tokio_tls::TlsStream,
    };

    #[test]
    fn tls_server() -> izanami::Result<()> {
        const IDENTITY: &[u8] = include_bytes!("../test/identity.pfx");
        const CERTIFICATE: &[u8] = include_bytes!("../test/server-crt.pem");

        let listener = TcpListener::bind(&"127.0.0.1:0".parse()?)?;
        let local_addr = listener.local_addr()?;

        let acceptor = TlsAcceptor::from_pkcs12(IDENTITY, "mypass")?;

        let runtime = Runtime::new()?;
        let mut serve = Server::bind(listener)?
            .accept(acceptor)
            .runtime(runtime)
            .launch(super::Echo)?;

        let connector = TlsConnector::builder()
            .add_root_certificate(Certificate::from_pem(CERTIFICATE)?)
            .build()?
            .into();

        let client = Client::builder() //
            .build(TestConnect {
                local_addr,
                connector,
            });

        let response = serve //
            .block_on(
                client.request(
                    Request::get("http://localhost/") //
                        .body(Body::empty())?,
                ),
            )?;
        assert_eq!(response.status(), 200);

        let body = serve.block_on(
            response
                .into_body() //
                .concat2(),
        )?;
        assert_eq!(body.into_bytes(), "hello");

        drop(client);
        serve.shutdown()
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
        izanami::{tls::openssl::SslAcceptor, Server},
        openssl::{
            pkey::PKey,
            rsa::Rsa,
            ssl::{SslConnector, SslMethod, SslVerifyMode},
            x509::X509,
        },
        std::{io, net::SocketAddr},
        tokio::{
            net::{TcpListener, TcpStream}, //
            runtime::current_thread::Runtime,
        },
        tokio_openssl::{SslConnectorExt, SslStream},
    };

    #[test]
    fn tls_server() -> izanami::Result<()> {
        const CERTIFICATE: &[u8] = include_bytes!("../test/server-crt.pem");
        const PRIVATE_KEY: &[u8] = include_bytes!("../test/server-key.pem");

        let listener = TcpListener::bind(&"127.0.0.1:0".parse()?)?;
        let local_addr = listener.local_addr()?;

        let cert = X509::from_pem(CERTIFICATE)?;
        let pkey = PKey::from_rsa(Rsa::private_key_from_pem(PRIVATE_KEY)?)?;

        let acceptor = SslAcceptor::new(&cert, &pkey)?;

        let runtime = Runtime::new()?;
        let mut serve = Server::bind(listener)?
            .accept(acceptor)
            .runtime(runtime)
            .launch(super::Echo)?;

        let connector = {
            let mut builder = SslConnector::builder(SslMethod::tls())?;
            builder.set_verify(SslVerifyMode::NONE);
            builder.set_certificate(&cert)?;
            builder.set_private_key(&pkey)?;
            builder.build()
        };

        let client = Client::builder() //
            .build(TestConnect {
                local_addr,
                connector,
            });

        let response = serve //
            .block_on(
                client.request(
                    Request::get("http://localhost/") //
                        .body(Body::empty())?,
                ),
            )?;
        assert_eq!(response.status(), 200);

        let body = serve.block_on(
            response
                .into_body() //
                .concat2(),
        )?;
        assert_eq!(body.into_bytes(), "hello");

        drop(client);
        serve.shutdown()
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
        failure::format_err,
        futures::{Future, Stream},
        http::Request,
        hyper::{
            client::{
                connect::{Connect, Connected, Destination},
                Client,
            },
            Body,
        },
        izanami::{tls::rustls::TlsAcceptor, Server},
        rustls::{
            ClientConfig, //
            ClientSession,
            KeyLogFile,
            NoClientAuth,
            ServerConfig,
        },
        std::sync::Arc,
        std::{
            io::{self, BufReader},
            net::SocketAddr,
        },
        tokio::{
            net::{TcpListener, TcpStream}, //
            runtime::current_thread::Runtime,
        },
        tokio_rustls::{TlsConnector, TlsStream},
        webpki::{DNSName, DNSNameRef},
    };

    #[test]
    #[ignore]
    fn tls_server() -> izanami::Result<()> {
        const CERTIFICATE: &[u8] = include_bytes!("../test/server-crt.pem");
        const PRIVATE_KEY: &[u8] = include_bytes!("../test/server-key.pem");

        let listener = TcpListener::bind(&"127.0.0.1:0".parse()?)?;
        let local_addr = listener.local_addr()?;

        let certs = {
            let mut reader = BufReader::new(io::Cursor::new(CERTIFICATE));
            rustls::internal::pemfile::certs(&mut reader)
                .map_err(|_| format_err!("failed to read certificate file"))?
        };

        let priv_key = {
            let mut reader = BufReader::new(io::Cursor::new(PRIVATE_KEY));
            let rsa_keys = rustls::internal::pemfile::rsa_private_keys(&mut reader)
                .map_err(|_| format_err!("failed to read private key file as RSA"))?;
            rsa_keys
                .into_iter()
                .next()
                .ok_or_else(|| format_err!("invalid private key"))?
        };

        let acceptor: TlsAcceptor = {
            let mut config = ServerConfig::new(NoClientAuth::new());
            config.key_log = Arc::new(KeyLogFile::new());
            config.set_single_cert(certs.clone(), priv_key.clone())?;
            config.into()
        };

        let runtime = Runtime::new()?;
        let mut serve = Server::bind(listener)?
            .accept(acceptor)
            .runtime(runtime)
            .launch(super::Echo)?;

        let connector = {
            let mut config = ClientConfig::new();
            config.set_single_client_cert(certs, priv_key);
            Arc::new(config).into()
        };

        let client = Client::builder() //
            .build(TestConnect {
                local_addr,
                connector,
                domain: DNSNameRef::try_from_ascii_str("localhost")
                    .map_err(|_| format_err!("invalid DNS name"))?
                    .to_owned(),
            });

        let response = serve //
            .block_on(
                client.request(
                    Request::get("http://localhost/") //
                        .body(Body::empty())?,
                ),
            )?;
        assert_eq!(response.status(), 200);

        let body = serve.block_on(
            response
                .into_body() //
                .concat2(),
        )?;
        assert_eq!(body.into_bytes(), "hello");

        drop(client);
        serve.shutdown()
    }

    struct TestConnect {
        local_addr: SocketAddr,
        connector: TlsConnector,
        domain: DNSName,
    }

    impl Connect for TestConnect {
        type Transport = TlsStream<TcpStream, ClientSession>;
        type Error = io::Error;
        type Future = Box<
            dyn Future<Item = (Self::Transport, Connected), Error = Self::Error> + Send + 'static,
        >;

        fn connect(&self, _: Destination) -> Self::Future {
            let connector = self.connector.clone();
            let domain = self.domain.clone();
            Box::new(
                TcpStream::connect(&self.local_addr)
                    .and_then(move |stream| connector.connect(domain.as_ref(), stream)) //
                    .map(|stream| (stream, Connected::new())),
            )
        }
    }
}
