//! The implementation of `StreamService` using a `Stream`.

use {
    crate::BoxedStdError,
    futures::{Async, Future, Poll, Stream},
    hyper::server::conn::Http,
    izanami_net::tcp::AddrIncoming as TcpAddrIncoming,
    izanami_service::Service,
    std::{io, net::ToSocketAddrs},
    tokio::io::{AsyncRead, AsyncWrite},
};

#[cfg(unix)]
use {izanami_net::unix::AddrIncoming as UnixAddrIncoming, std::path::Path};

/// A `Service` that produces pairs of an incoming stream and an associated service.
#[derive(Debug)]
pub struct Incoming<I: Stream, S> {
    incoming: I,
    stream: Option<I::Item>,
    make_service: S,
    protocol: Http,
}

impl<I> Incoming<I, ()>
where
    I: Stream,
    I::Error: Into<BoxedStdError>,
{
    pub fn builder(incoming: I) -> Builder<I> {
        Builder {
            incoming,
            protocol: Http::new(),
        }
    }
}

impl Incoming<TcpAddrIncoming, ()> {
    pub fn bind_tcp<A>(addr: A) -> io::Result<Builder<TcpAddrIncoming>>
    where
        A: ToSocketAddrs,
    {
        Ok(Incoming::builder(TcpAddrIncoming::bind(addr)?))
    }
}

#[cfg(unix)]
impl Incoming<UnixAddrIncoming, ()> {
    pub fn bind_unix<P>(path: P) -> io::Result<Builder<UnixAddrIncoming>>
    where
        P: AsRef<Path>,
    {
        Ok(Incoming::builder(UnixAddrIncoming::bind(path)?))
    }
}

impl<I, S> Service<()> for Incoming<I, S>
where
    I: Stream,
    I::Item: AsyncRead + AsyncWrite,
    I::Error: Into<BoxedStdError>,
    S: Service<()>,
    S::Error: Into<BoxedStdError>,
{
    type Response = (S::Response, I::Item, Http);
    type Error = BoxedStdError;
    type Future = IncomingFuture<I, S>;

    fn poll_ready(&mut self) -> Poll<(), Self::Error> {
        if self.stream.is_some() {
            return Ok(Async::Ready(()));
        }
        match futures::try_ready!(self.incoming.poll().map_err(Into::into)) {
            Some(stream) => {
                self.stream = Some(stream);
                Ok(Async::Ready(()))
            }
            None => Err(failure::format_err!("incoming closed").compat().into()),
        }
    }

    fn call(&mut self, _: ()) -> Self::Future {
        let stream = self.stream.take().expect("empty stream");
        let make_service_future = self.make_service.call(());
        IncomingFuture {
            make_service_future,
            stream: Some(stream),
            protocol: Some(self.protocol.clone()),
        }
    }
}

#[doc(hidden)]
#[allow(missing_debug_implementations, clippy::type_complexity)]
pub struct IncomingFuture<I, S>
where
    I: Stream,
    I::Item: AsyncRead + AsyncWrite,
    I::Error: Into<BoxedStdError>,
    S: Service<()>,
    S::Error: Into<BoxedStdError>,
{
    make_service_future: S::Future,
    stream: Option<I::Item>,
    protocol: Option<Http>,
}

impl<I, S> Future for IncomingFuture<I, S>
where
    I: Stream,
    I::Item: AsyncRead + AsyncWrite,
    I::Error: Into<BoxedStdError>,
    S: Service<()>,
    S::Error: Into<BoxedStdError>,
{
    type Item = (S::Response, I::Item, Http);
    type Error = BoxedStdError;

    #[inline]
    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let service = futures::try_ready!(self.make_service_future.poll().map_err(Into::into));
        let stream = self
            .stream
            .take()
            .expect("the future has already been polled.");
        let protocol = self
            .protocol
            .take()
            .expect("the future has already been polled");
        Ok(Async::Ready((service, stream, protocol)))
    }
}

/// A builder of `Server` using `Incoming` as streamed service.
#[derive(Debug)]
pub struct Builder<I> {
    incoming: I,
    protocol: Http,
}

impl<I> Builder<I>
where
    I: Stream,
    I::Item: AsyncRead + AsyncWrite,
    I::Error: Into<BoxedStdError>,
{
    /// Specifies that the server uses only HTTP/1.
    pub fn http1_only(mut self) -> Self {
        self.protocol.http1_only(true);
        self
    }

    /// Specifies that the server uses only HTTP/2.
    pub fn http2_only(mut self) -> Self {
        self.protocol.http2_only(true);
        self
    }

    /// Specifies a `make_service` to serve incoming connections.
    pub fn serve<S>(self, make_service: S) -> Incoming<I, S>
    where
        S: Service<()>,
        S::Error: Into<BoxedStdError>,
    {
        Incoming {
            make_service,
            incoming: self.incoming,
            stream: None,
            protocol: self.protocol,
        }
    }
}
