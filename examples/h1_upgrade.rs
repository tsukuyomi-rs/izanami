use {
    futures::{Async, Future, Poll},
    http::{Response, StatusCode},
    izanami::{
        http::{
            body::HttpBody,
            upgrade::{HttpUpgrade, Upgraded},
        },
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

enum WithUpgrade<T, U> {
    Data(T),
    Upgrade(U),
}

impl<T, U> HttpBody for WithUpgrade<T, U>
where
    T: HttpBody,
{
    type Data = T::Data;
    type Error = T::Error;

    fn poll_data(&mut self) -> Poll<Option<Self::Data>, Self::Error> {
        match self {
            WithUpgrade::Data(ref mut data) => data.poll_data(),
            WithUpgrade::Upgrade(..) => Ok(Async::Ready(None)),
        }
    }

    fn poll_trailers(&mut self) -> Poll<Option<http::HeaderMap>, Self::Error> {
        match self {
            WithUpgrade::Data(ref mut data) => data.poll_trailers(),
            WithUpgrade::Upgrade(..) => Ok(Async::Ready(None)),
        }
    }

    fn is_end_stream(&self) -> bool {
        match self {
            WithUpgrade::Data(ref data) => data.is_end_stream(),
            WithUpgrade::Upgrade(..) => true,
        }
    }

    fn content_length(&self) -> Option<u64> {
        match self {
            WithUpgrade::Data(ref data) => data.content_length(),
            WithUpgrade::Upgrade(..) => None,
        }
    }

    fn size_hint(&self) -> tokio_buf::SizeHint {
        match self {
            WithUpgrade::Data(ref data) => data.size_hint(),
            WithUpgrade::Upgrade(..) => Default::default(),
        }
    }
}

impl<T, U, I> HttpUpgrade<I> for WithUpgrade<T, U>
where
    U: HttpUpgrade<I>,
{
    type Upgraded = U::Upgraded;
    type Error = U::Error;

    fn upgrade(self, stream: I) -> Result<Self::Upgraded, I> {
        match self {
            WithUpgrade::Upgrade(cx) => cx.upgrade(stream),
            WithUpgrade::Data(..) => Err(stream),
        }
    }
}

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

impl<I> Upgraded for FooBarConnection<I>
where
    I: AsyncRead + AsyncWrite,
{
    type Error = std::io::Error;

    fn poll_done(&mut self) -> Poll<(), Self::Error> {
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

    fn graceful_shutdown(&mut self) {}
}

fn bad_request<T, U>(body: T) -> Response<WithUpgrade<T, U>> {
    let mut res = Response::new(WithUpgrade::Data(body));
    *res.status_mut() = StatusCode::BAD_REQUEST;
    res
}

fn main() -> io::Result<()> {
    let server = Server::new(
        AddrIncoming::bind("127.0.0.1:5000")? // Stream<Item = AddrStream>
            .into_service() // <-- Stream -> Service<()>
            .with_adaptors()
            .map(|stream| {
                H1Connection::build(stream) //
                    .finish(service_fn(move |req: H1Request| -> io::Result<_> {
                        match req.headers().get(http::header::CONNECTION) {
                            Some(h) if h == "upgrade" => (),
                            Some(..) => {
                                return Ok(bad_request(
                                    "the header field `connection' must be equal to 'upgrade'.",
                                ));
                            }
                            None => return Ok(bad_request("missing header field: `connection'")),
                        }
                        match req.headers().get(http::header::UPGRADE) {
                            Some(h) if h == "foobar" => (),
                            Some(..) => {
                                return Ok(bad_request(
                                    "the header field `upgrade' must be equal to 'foobar'.",
                                ));
                            }
                            None => return Ok(bad_request("missing header field: `upgrade'")),
                        }
                        let response = Response::builder()
                            .status(101)
                            .header("upgrade", "foobar")
                            .body(WithUpgrade::Upgrade(FooBar(())))
                            .unwrap();
                        Ok(response)
                    }))
            }),
    )
    .map_err(|e| eprintln!("server error: {}", e));

    izanami::rt::run(server);
    Ok(())
}
