//! The implementation of `StreamService` using a `Stream`.

use {
    crate::{server::Protocol, BoxedStdError},
    futures::{Async, Future, Poll, Stream},
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
    state: IncomingState<I::Item>,
    make_service: S,
}

#[derive(Debug)]
enum IncomingState<I> {
    Pending,
    Ready(I),
}

impl<I> Incoming<I, ()>
where
    I: Stream,
    I::Error: Into<BoxedStdError>,
{
    pub fn builder(incoming: I) -> Builder<I> {
        Builder { incoming }
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

impl<I, S> Service<Protocol> for Incoming<I, S>
where
    I: Stream,
    I::Item: AsyncRead + AsyncWrite,
    I::Error: Into<BoxedStdError>,
    S: Service<()>,
    S::Error: Into<BoxedStdError>,
{
    type Response = (S::Response, I::Item, Protocol);
    type Error = BoxedStdError;
    type Future = IncomingFuture<I, S>;

    fn poll_ready(&mut self) -> Poll<(), Self::Error> {
        loop {
            self.state = match self.state {
                IncomingState::Pending => {
                    futures::try_ready!(self.make_service.poll_ready().map_err(Into::into));
                    let stream = futures::try_ready!(self.incoming.poll().map_err(Into::into))
                        .ok_or_else(|| failure::format_err!("incoming closed").compat())?;
                    IncomingState::Ready(stream)
                }
                IncomingState::Ready(..) => return Ok(Async::Ready(())),
            };
        }
    }

    fn call(&mut self, protocol: Protocol) -> Self::Future {
        let stream = match std::mem::replace(&mut self.state, IncomingState::Pending) {
            IncomingState::Ready(stream) => stream,
            IncomingState::Pending => panic!("the service is not ready"),
        };
        let make_service_future = self.make_service.call(());
        IncomingFuture {
            make_service_future,
            stream: Some(stream),
            protocol: Some(protocol),
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
    protocol: Option<Protocol>,
}

impl<I, S> Future for IncomingFuture<I, S>
where
    I: Stream,
    I::Item: AsyncRead + AsyncWrite,
    I::Error: Into<BoxedStdError>,
    S: Service<()>,
    S::Error: Into<BoxedStdError>,
{
    type Item = (S::Response, I::Item, Protocol);
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
}

impl<I> Builder<I>
where
    I: Stream,
    I::Item: AsyncRead + AsyncWrite,
    I::Error: Into<BoxedStdError>,
{
    /// Specifies a `make_service` to serve incoming connections.
    pub fn serve<S>(self, make_service: S) -> Incoming<I, S>
    where
        S: Service<()>,
        S::Error: Into<BoxedStdError>,
    {
        Incoming {
            make_service,
            incoming: self.incoming,
            state: IncomingState::Pending,
        }
    }
}
