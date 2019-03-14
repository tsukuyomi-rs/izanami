use {
    futures::{Async, Future, Poll},
    http::{Response, StatusCode},
    izanami::{
        http::upgrade::{HttpUpgrade, MaybeUpgrade},
        net::tcp::AddrIncoming,
        server::{
            h1::{H1Connection, HttpRequest as H1Request},
            Server,
        }, //
        service::{ext::ServiceExt, service_fn, stream::StreamExt},
    },
    std::io,
    tokio::io::{AsyncRead, AsyncWrite},
};

struct FooBar(());

impl<I> HttpUpgrade<I> for FooBar
where
    I: AsyncRead + AsyncWrite,
{
    type Upgraded = FooBarConnection<I>;
    type Error = std::io::Error;

    fn upgrade(self, stream: I) -> Result<Self::Upgraded, I> {
        Ok(FooBarConnection {
            state: State::Reading(tokio::io::read_exact(stream, vec![0; 7])),
        })
    }
}

struct FooBarConnection<I> {
    state: State<I>,
}

enum State<I> {
    Reading(tokio::io::ReadExact<I, Vec<u8>>),
    Writing(tokio::io::WriteAll<I, &'static [u8]>),
    Closing(tokio::io::Shutdown<I>),
    Closed,
}

impl<I> Future for FooBarConnection<I>
where
    I: AsyncRead + AsyncWrite,
{
    type Item = ();
    type Error = std::io::Error;

    fn poll(&mut self) -> Poll<(), Self::Error> {
        loop {
            self.state = match self.state {
                State::Reading(ref mut read_exact) => {
                    let (stream, buf) = futures::try_ready!(read_exact.poll());
                    println!("server[foobar] recv: {:?}", std::str::from_utf8(&buf));
                    State::Writing(tokio::io::write_all(stream, b"bar=foo"))
                }
                State::Writing(ref mut write_all) => {
                    let (stream, _) = futures::try_ready!(write_all.poll());
                    println!("server[foobar] sent");
                    State::Closing(tokio::io::shutdown(stream))
                }
                State::Closing(ref mut shutdown) => {
                    let _stream = futures::try_ready!(shutdown.poll());
                    println!("server[foobar] closed");
                    State::Closed
                }
                State::Closed => return Ok(Async::Ready(())),
            };
        }
    }
}

fn main() -> io::Result<()> {
    let server = Server::new(
        AddrIncoming::bind("127.0.0.1:5000")? // Stream<Item = AddrStream>
            .into_service() // <-- Stream -> Service<()>
            .with_adaptors()
            .map(|stream| {
                H1Connection::build(stream) //
                    .finish(service_fn(move |req: H1Request| -> io::Result<_> {
                        let err_msg = match req.headers().get(http::header::UPGRADE) {
                            Some(h) if h == "foobar" => None,
                            Some(..) => {
                                Some("the header field `upgrade' must be equal to 'foobar'.")
                            }
                            None => Some("missing header field: `upgrade'"),
                        };

                        if let Some(err_msg) = err_msg {
                            let mut res = Response::new(MaybeUpgrade::Data(err_msg));
                            *res.status_mut() = StatusCode::BAD_REQUEST;
                            return Ok(res);
                        }

                        // When the response has a status code `101 Switching Protocols`, the connection
                        // upgrades the protocol using the associated response body.
                        let response = Response::builder()
                            .status(101)
                            .header("upgrade", "foobar")
                            .body(MaybeUpgrade::Upgrade(FooBar(())))
                            .unwrap();

                        Ok(response)
                    }))
            }),
    )
    .map_err(|e| eprintln!("server error: {}", e));

    izanami::rt::run(server);
    Ok(())
}
