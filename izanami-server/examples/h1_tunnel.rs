#![recursion_limit = "128"]

use {
    futures::Future,
    http::Response,
    izanami_server::{
        net::tcp::AddrIncoming,
        protocol::h1::{H1Request, H1},
        upgrade::MaybeUpgrade,
        Server,
    },
    izanami_service::{service_fn, ServiceExt},
    tokio::io,
};

fn main() -> io::Result<()> {
    let mut runtime = tokio::runtime::Runtime::new()?;

    let (resolver, background) = resolve::Resolver::default();
    runtime.spawn(background);

    let server = Server::new(
        AddrIncoming::bind("127.0.0.1:5000")? //
            .service_map(move |stream| {
                let resolver = resolver.clone();
                H1::new().serve(
                    stream,
                    service_fn(move |req: H1Request| {
                        let hostname = req.uri().host().unwrap_or("localhost");
                        let port = req.uri().port_u16().unwrap_or(80);
                        resolver
                            .lookup_and_connect(hostname, port)
                            .then(|res| -> io::Result<_> {
                                eprintln!("lookup result: {:?}", res);
                                let response = match res {
                                    Ok(server) => Response::builder()
                                        .body(MaybeUpgrade::Upgrade(proxy::Proxy { server }))
                                        .unwrap(),
                                    Err(err) => Response::builder()
                                        .status(400)
                                        .body(MaybeUpgrade::Data(format!("proxy error: {}", err)))
                                        .unwrap(),
                                };
                                Ok(response)
                            })
                    }),
                )
            }),
    )
    .map_err(|e| eprintln!("server error: {}", e));

    tokio::run(server);
    Ok(())
}

mod resolve {
    use {
        futures::Future,
        std::net::SocketAddr,
        tokio::{io, net::TcpStream},
        trust_dns_resolver::{
            config::{ResolverConfig, ResolverOpts},
            AsyncResolver,
        },
    };

    #[derive(Clone)]
    pub struct Resolver {
        resolver: AsyncResolver,
    }

    impl Resolver {
        pub fn default() -> (Self, impl Future<Item = (), Error = ()>) {
            let (resolver, background) =
                AsyncResolver::new(ResolverConfig::default(), ResolverOpts::default());
            (Self { resolver }, background)
        }

        pub fn lookup_and_connect(
            &self,
            hostname: &str,
            port: u16,
        ) -> impl Future<Item = TcpStream, Error = io::Error> {
            eprintln!("lookup IP address for {}", hostname);
            self.resolver
                .lookup_ip(hostname)
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))
                .and_then(move |lookup_ips| {
                    lookup_ips
                        .iter()
                        .next()
                        .map(|ip| SocketAddr::from((ip, port)))
                        .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "empty lookup IPs"))
                })
                .and_then(|addr| {
                    eprintln!("found lookup address: {}", addr);
                    TcpStream::connect(&addr)
                })
        }
    }
}

mod proxy {
    use {
        futures::{Async, Future, Poll},
        izanami_server::{upgrade::HttpUpgrade, Connection},
        std::sync::{Arc, Mutex},
        tokio::io,
    };

    pub struct Proxy<S> {
        pub server: S,
    }

    impl<S, C> HttpUpgrade<C> for Proxy<S>
    where
        C: io::AsyncRead + io::AsyncWrite,
        S: io::AsyncRead + io::AsyncWrite,
    {
        type Upgraded = ProxyConnection<C, S>;
        type Error = io::Error;

        fn upgrade(self, client: C) -> Result<Self::Upgraded, C> {
            let (read_client, write_client) = make_half(client);
            let (read_server, write_server) = make_half(self.server);
            Ok(ProxyConnection {
                inner: State::Copy(tokio::io::copy(read_client, write_server))
                    .join(State::Copy(tokio::io::copy(read_server, write_client))),
            })
        }
    }

    #[allow(clippy::type_complexity)]
    pub struct ProxyConnection<C, S>
    where
        C: io::AsyncRead + io::AsyncWrite,
        S: io::AsyncRead + io::AsyncWrite,
    {
        inner: futures::future::Join<
            State<ReadHalf<C>, WriteHalf<S>>,
            State<ReadHalf<S>, WriteHalf<C>>,
        >,
    }

    impl<C, S> Connection for ProxyConnection<C, S>
    where
        C: io::AsyncRead + io::AsyncWrite,
        S: io::AsyncRead + io::AsyncWrite,
    {
        type Error = io::Error;

        fn poll_close(&mut self) -> Poll<(), Self::Error> {
            self.inner.poll().map(|x| x.map(|_| ()))
        }

        fn graceful_shutdown(&mut self) {}
    }

    // ===== State =====

    enum State<R, W> {
        Copy(tokio::io::Copy<R, W>),
        Shutdown(tokio::io::Shutdown<W>),
        Done,
    }

    impl<R, W> Future for State<R, W>
    where
        R: io::AsyncRead,
        W: io::AsyncWrite,
    {
        type Item = ();
        type Error = io::Error;

        fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
            loop {
                *self = match self {
                    State::Copy(copy) => {
                        let (_, _, writer) = futures::try_ready!(copy.poll());
                        State::Shutdown(tokio::io::shutdown(writer))
                    }
                    State::Shutdown(shutdown) => {
                        let _writer = futures::try_ready!(shutdown.poll());
                        State::Done
                    }
                    State::Done => return Ok(Async::Ready(())),
                };
            }
        }
    }

    fn make_half<T>(stream: T) -> (ReadHalf<T>, WriteHalf<T>)
    where
        T: io::AsyncRead + io::AsyncWrite,
    {
        let inner = Arc::new(Mutex::new(stream));
        (ReadHalf(inner.clone()), WriteHalf(inner.clone()))
    }

    // ===== ReadHalf =====

    struct ReadHalf<T: io::AsyncRead>(Arc<Mutex<T>>);

    impl<T: io::AsyncRead> io::Read for ReadHalf<T> {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            self.0.lock().unwrap().read(buf)
        }
    }

    impl<T: io::AsyncRead> io::AsyncRead for ReadHalf<T> {}

    // ===== WriteHalf =====

    struct WriteHalf<T: io::AsyncWrite>(Arc<Mutex<T>>);

    impl<T: io::AsyncWrite> io::Write for WriteHalf<T> {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().write(buf)
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl<T: io::AsyncWrite> io::AsyncWrite for WriteHalf<T> {
        fn shutdown(&mut self) -> futures::Poll<(), io::Error> {
            self.0.lock().unwrap().shutdown()
        }
    }
}
