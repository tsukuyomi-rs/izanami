//! The implementation of `StreamService` using a `Stream`.

use {
    crate::{
        request::{HttpRequest, RequestBody},
        BoxedStdError, Builder, Server,
    },
    futures::{Async, Future, IntoFuture, Poll, Stream},
    izanami_http::{HttpBody, HttpService},
    izanami_net::{
        tcp::AddrStream as TcpAddrStream,
        tls::{MakeTlsTransport, NoTls},
    },
    izanami_service::{IntoService, Service, StreamService},
    std::{
        io,
        net::{SocketAddr, ToSocketAddrs},
    },
};

#[cfg(unix)]
use {
    izanami_net::unix::AddrStream as UnixAddrStream, //
    std::path::Path,
};

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
pub struct Incoming<S, I, T = NoTls> {
    make_service: S,
    incoming: I,
    tls: T,
}

type TcpIncoming<S, T = NoTls> = Incoming<S, izanami_net::tcp::AddrIncoming, T>;

#[cfg(unix)]
type UnixIncoming<S, T = NoTls> = Incoming<S, izanami_net::unix::AddrIncoming, T>;

impl<S, I, T> StreamService for Incoming<S, I, T>
where
    S: MakeHttpService<T::Transport>,
    I: Stream,
    I::Error: Into<BoxedStdError>,
    T: MakeTlsTransport<I::Item>,
    T::Error: Into<BoxedStdError>,
{
    type Response = (S::Service, T::Transport);
    type Error = BoxedStdError;
    type Future = IncomingFuture<S::Future, T::Future>;

    fn poll_next_service(&mut self) -> Poll<Option<Self::Future>, Self::Error> {
        let stream = match futures::try_ready!(self.incoming.poll().map_err(Into::into)) {
            Some(stream) => stream,
            None => return Ok(Async::Ready(None)),
        };
        let make_service_future = self.make_service.make_service();
        let make_transport_future = self.tls.make_transport(stream);
        Ok(Async::Ready(Some(IncomingFuture {
            inner: (
                make_service_future.map_err(Into::into as fn(_) -> _),
                make_transport_future.map_err(Into::into as fn(_) -> _),
            )
                .into_future(),
        })))
    }
}

#[doc(hidden)]
#[allow(missing_debug_implementations, clippy::type_complexity)]
pub struct IncomingFuture<FutS, FutT>
where
    FutS: Future,
    FutS::Error: Into<BoxedStdError>,
    FutT: Future,
    FutT::Error: Into<BoxedStdError>,
{
    inner: futures::future::Join<
        futures::future::MapErr<FutS, fn(FutS::Error) -> BoxedStdError>,
        futures::future::MapErr<FutT, fn(FutT::Error) -> BoxedStdError>, //
    >,
}

impl<FutS, FutT> Future for IncomingFuture<FutS, FutT>
where
    FutS: Future,
    FutS::Item: IntoHttpService<FutT::Item>,
    FutS::Error: Into<BoxedStdError>,
    FutT: Future,
    FutT::Error: Into<BoxedStdError>,
{
    type Item = (
        <FutS::Item as IntoHttpService<FutT::Item>>::Service,
        FutT::Item,
    );
    type Error = BoxedStdError;

    #[inline]
    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let (into_service, transport) = futures::try_ready!(self.inner.poll());
        let service = into_service.into_service(&transport);
        Ok(Async::Ready((service, transport)))
    }
}

impl<I, T, Sig> Builder<Incoming<(), I, T>, Sig>
where
    I: Stream,
    T: MakeTlsTransport<I::Item>,
    T::Error: Into<BoxedStdError>,
{
    /// Specifies a `make_service` to serve incoming connections.
    pub fn serve<S>(self, make_service: S) -> Builder<Incoming<S, I, T>, Sig>
    where
        S: MakeHttpService<T::Transport>,
    {
        Builder {
            stream_service: Incoming {
                make_service,
                incoming: self.stream_service.incoming,
                tls: self.stream_service.tls,
            },
            protocol: self.protocol,
            shutdown_signal: self.shutdown_signal,
        }
    }
}

impl Server<()> {
    /// Create a `Builder` using a TCP listener bound to the specified address.
    pub fn bind_tcp<A, T>(addr: A, tls: T) -> io::Result<Builder<TcpIncoming<(), T>>>
    where
        A: ToSocketAddrs,
        T: MakeTlsTransport<TcpAddrStream>,
        T::Error: Into<BoxedStdError>,
    {
        let incoming = izanami_net::tcp::AddrIncoming::bind(addr)?;
        Ok(Server::builder(Incoming {
            make_service: (),
            incoming,
            tls,
        }))
    }

    /// Create a `Builder` using a Unix domain socket listener bound to the specified socket path.
    #[cfg(unix)]
    pub fn bind_unix<P, T>(path: P, tls: T) -> io::Result<Builder<UnixIncoming<(), T>>>
    where
        P: AsRef<Path>,
        T: MakeTlsTransport<UnixAddrStream>,
        T::Error: Into<BoxedStdError>,
    {
        let incoming = izanami_net::unix::AddrIncoming::bind(path)?;
        Ok(Server::builder(Incoming {
            make_service: (),
            incoming,
            tls,
        }))
    }
}

impl<S, T, Sig> Server<TcpIncoming<S, T>, Sig> {
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
