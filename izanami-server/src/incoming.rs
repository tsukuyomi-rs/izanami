//! The implementation of `StreamService` using a `Stream`.

use {
    crate::{
        request::{HttpRequest, RequestBody},
        BoxedStdError, Server,
    },
    futures::{Async, Future, Poll, Stream},
    hyper::server::conn::Http,
    izanami_http::{HttpBody, HttpService},
    izanami_net::tcp::AddrIncoming as TcpAddrIncoming,
    izanami_service::{IntoService, Service, StreamService},
    std::{
        io,
        net::{SocketAddr, ToSocketAddrs},
    },
    tokio::io::{AsyncRead, AsyncWrite},
};

#[cfg(unix)]
use {izanami_net::unix::AddrIncoming as UnixAddrIncoming, std::path::Path};

/// An asynchronous factory of `HttpService`s.
pub trait MakeHttpService<T>: self::imp::MakeHttpServiceSealed<T> {
    type ResponseBody: HttpBody;
    type Error: Into<BoxedStdError>;
    type Service: HttpService<
        RequestBody, //
        ResponseBody = Self::ResponseBody,
        Error = Self::Error,
    >;
    type IntoService: IntoHttpService<
        T,
        ResponseBody = Self::ResponseBody,
        Error = Self::Error,
        Service = Self::Service,
    >;
    type MakeError: Into<BoxedStdError>;
    type Future: Future<Item = Self::IntoService, Error = Self::MakeError>;

    #[doc(hidden)]
    fn poll_ready(&mut self) -> Poll<(), Self::MakeError> {
        Ok(Async::Ready(()))
    }

    fn make_service(&mut self) -> Self::Future;
}

impl<S, T> MakeHttpService<T> for S
where
    S: Service<()>,
    S::Response: IntoHttpService<T>,
    S::Error: Into<BoxedStdError>,
{
    type ResponseBody = <S::Response as IntoHttpService<T>>::ResponseBody;
    type Error = <S::Response as IntoHttpService<T>>::Error;
    type Service = <S::Response as IntoHttpService<T>>::Service;
    type IntoService = S::Response;
    type MakeError = S::Error;
    type Future = S::Future;

    #[inline]
    fn poll_ready(&mut self) -> Poll<(), Self::MakeError> {
        Service::poll_ready(self)
    }

    #[inline]
    fn make_service(&mut self) -> Self::Future {
        Service::call(self, ())
    }
}

/// A value to be converted into an `HttpService`.
pub trait IntoHttpService<T>: self::imp::IntoHttpServiceSealed<T> {
    type ResponseBody: HttpBody;
    type Error: Into<BoxedStdError>;
    type Service: HttpService<RequestBody, ResponseBody = Self::ResponseBody, Error = Self::Error>;

    fn into_service(self, target: &T) -> Self::Service;
}

impl<S, T, Bd, Err, Svc> IntoHttpService<T> for S
where
    S: for<'a> IntoService<
        &'a T,
        HttpRequest,
        Response = http::Response<Bd>,
        Error = Err,
        Service = Svc,
    >,
    Bd: HttpBody,
    Err: Into<BoxedStdError>,
    Svc: Service<HttpRequest, Response = http::Response<Bd>, Error = Err>,
{
    type ResponseBody = Bd;
    type Error = Err;
    type Service = Svc;

    #[inline]
    fn into_service(self, target: &T) -> Self::Service {
        IntoService::into_service(self, target)
    }
}

/// A `StreamService` that uses a `Stream` of I/O objects.
#[derive(Debug)]
pub struct Incoming<S, I> {
    make_service: S,
    incoming: I,
    protocol: Http,
}

impl<S, I> StreamService for Incoming<S, I>
where
    S: MakeHttpService<I::Item>,
    I: Stream,
    I::Item: AsyncRead + AsyncWrite,
    I::Error: Into<BoxedStdError>,
{
    type Response = (S::Service, I::Item, Http);
    type Error = BoxedStdError;
    type Future = IncomingFuture<S::Future, I::Item>;

    fn poll_next_service(&mut self) -> Poll<Option<Self::Future>, Self::Error> {
        let stream = match futures::try_ready!(self.incoming.poll().map_err(Into::into)) {
            Some(stream) => stream,
            None => return Ok(Async::Ready(None)),
        };
        let make_service_future = self.make_service.make_service();
        Ok(Async::Ready(Some(IncomingFuture {
            make_service_future,
            stream: Some(stream),
            protocol: Some(self.protocol.clone()),
        })))
    }
}

#[doc(hidden)]
#[allow(missing_debug_implementations, clippy::type_complexity)]
pub struct IncomingFuture<Fut, I>
where
    Fut: Future,
    Fut::Error: Into<BoxedStdError>,
    I: AsyncRead + AsyncWrite,
{
    make_service_future: Fut,
    stream: Option<I>,
    protocol: Option<Http>,
}

impl<Fut, I> Future for IncomingFuture<Fut, I>
where
    Fut: Future,
    Fut::Item: IntoHttpService<I>,
    Fut::Error: Into<BoxedStdError>,
    I: AsyncRead + AsyncWrite,
{
    type Item = (<Fut::Item as IntoHttpService<I>>::Service, I, Http);
    type Error = BoxedStdError;

    #[inline]
    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let into_service = futures::try_ready!(self.make_service_future.poll().map_err(Into::into));
        let stream = self
            .stream
            .take()
            .expect("the future has already been polled.");
        let service = into_service.into_service(&stream);
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
    pub fn serve<S>(self, make_service: S) -> Server<Incoming<S, I>>
    where
        S: MakeHttpService<I::Item>,
    {
        Server::new(Incoming {
            make_service,
            incoming: self.incoming,
            protocol: self.protocol,
        })
    }
}

impl Server<()> {
    /// Create a `Builder` using a TCP listener bound to the specified address.
    pub fn bind_tcp<A>(addr: A) -> io::Result<Builder<TcpAddrIncoming>>
    where
        A: ToSocketAddrs,
    {
        Ok(Builder {
            incoming: izanami_net::tcp::AddrIncoming::bind(addr)?,
            protocol: Http::new(),
        })
    }

    /// Create a `Builder` using a Unix domain socket listener bound to the specified socket path.
    #[cfg(unix)]
    pub fn bind_unix<P>(path: P) -> io::Result<Builder<UnixAddrIncoming>>
    where
        P: AsRef<Path>,
    {
        Ok(Builder {
            incoming: izanami_net::unix::AddrIncoming::bind(path)?,
            protocol: Http::new(),
        })
    }
}

impl<S, Sig> Server<Incoming<S, TcpAddrIncoming>, Sig> {
    #[doc(hidden)]
    pub fn local_addr(&self) -> SocketAddr {
        self.stream_service.incoming.local_addr()
    }
}

mod imp {
    use super::*;

    pub trait MakeHttpServiceSealed<T> {}

    impl<S, T> MakeHttpServiceSealed<T> for S
    where
        S: Service<()>,
        S::Response: IntoHttpService<T>,
        S::Error: Into<BoxedStdError>,
    {
    }

    pub trait IntoHttpServiceSealed<T> {}

    impl<S, T, Bd, Err, Svc> IntoHttpServiceSealed<T> for S
    where
        S: for<'a> IntoService<
            &'a T,
            HttpRequest,
            Response = http::Response<Bd>,
            Error = Err,
            Service = Svc,
        >,
        Bd: HttpBody,
        Err: Into<BoxedStdError>,
        Svc: Service<HttpRequest, Response = http::Response<Bd>, Error = Err>,
    {
    }
}
