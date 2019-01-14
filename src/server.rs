//! A lightweight implementation of HTTP server based on Hyper.

pub mod conn;

use {
    self::conn::{Acceptor, DefaultTransport, Transport},
    crate::CritError,
    futures::{Future, Poll, Stream},
    http::{HeaderMap, Request, Response},
    hyper::{body::Payload as _Payload, server::conn::Http},
    izanami_http::{
        buf_stream::{BufStream, SizeHint},
        upgrade::Upgrade,
        HasTrailers,
    },
    izanami_service::{MakeServiceRef, Service},
    std::{
        fmt, //
        io,
        marker::PhantomData,
        net::SocketAddr,
        time::Duration,
    },
    tokio::{
        io::{AsyncRead, AsyncWrite},
        net::TcpListener,
    },
};

#[cfg(unix)]
use {std::path::Path, tokio::net::UnixListener};

/// A struct that represents the stream of chunks from client.
#[derive(Debug)]
pub struct RequestBody(Inner);

#[derive(Debug)]
enum Inner {
    Raw(hyper::Body),
    OnUpgrade(hyper::upgrade::OnUpgrade),
}

impl RequestBody {
    pub(crate) fn from_hyp(body: hyper::Body) -> Self {
        RequestBody(Inner::Raw(body))
    }
}

impl BufStream for RequestBody {
    type Item = hyper::Chunk;
    type Error = hyper::Error;

    fn poll_buf(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        match &mut self.0 {
            Inner::Raw(body) => body.poll_data(),
            Inner::OnUpgrade(..) => panic!("the request body has already been upgraded"),
        }
    }

    fn size_hint(&self) -> SizeHint {
        match &self.0 {
            Inner::Raw(body) => {
                let mut hint = SizeHint::new();
                if let Some(len) = body.content_length() {
                    hint.set_upper(len);
                    hint.set_lower(len);
                }
                hint
            }
            Inner::OnUpgrade(..) => panic!("the request body has already been upgraded"),
        }
    }
}

impl HasTrailers for RequestBody {
    fn poll_trailers(&mut self) -> Poll<Option<HeaderMap>, Self::Error> {
        match &mut self.0 {
            Inner::Raw(body) => body.poll_trailers(),
            Inner::OnUpgrade(..) => panic!("the request body has already been upgraded"),
        }
    }
}

impl Upgrade for RequestBody {
    type Upgraded = hyper::upgrade::Upgraded;
    type Error = hyper::Error;

    fn poll_upgrade(&mut self) -> Poll<Self::Upgraded, Self::Error> {
        loop {
            self.0 = match &mut self.0 {
                Inner::Raw(body) => {
                    let body = std::mem::replace(body, Default::default());
                    Inner::OnUpgrade(body.on_upgrade())
                }
                Inner::OnUpgrade(on_upgrade) => return on_upgrade.poll(),
            };
        }
    }
}

// ==== Server ====

/// A simple HTTP server that wraps the `hyper`'s server implementation.
pub struct Server<
    T = DefaultTransport<TcpListener>, //
    B = Threadpool,
> {
    transport: T,
    protocol: Http,
    _marker: PhantomData<B>,
}

impl<T, B> fmt::Debug for Server<T, B>
where
    T: Transport + fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Server")
            .field("transport", &self.transport)
            .field("protocol", &self.protocol)
            .finish()
    }
}

impl Server {
    /// Creates an HTTP server using a TCP listener.
    pub fn bind_tcp(addr: &SocketAddr) -> io::Result<Server<DefaultTransport<TcpListener>>> {
        let transport = TcpListener::bind(addr)?;
        Ok(Server::new(DefaultTransport::new(transport, ())))
    }

    /// Creates an HTTP server using a Unix domain socket listener.
    #[cfg(unix)]
    pub fn bind_uds(path: impl AsRef<Path>) -> io::Result<Server<DefaultTransport<UnixListener>>> {
        let transport = UnixListener::bind(path)?;
        Ok(Server::new(DefaultTransport::new(transport, ())))
    }
}

impl<T, B> Server<T, B>
where
    T: Transport,
{
    /// Create a `Server` from a specific `Transport`.
    pub fn new(transport: T) -> Self {
        Self {
            transport,
            protocol: Http::new(),
            _marker: PhantomData,
        }
    }

    /// Returns a reference to the inner transport.
    pub fn transport(&mut self) -> &mut T {
        &mut self.transport
    }

    /// Returns a reference to the HTTP-level configuration.
    pub fn protocol(&mut self) -> &mut Http {
        &mut self.protocol
    }

    /// Switches the backend to `CurrentThread`.
    pub fn current_thread(self) -> Server<T, CurrentThread> {
        Server {
            transport: self.transport,
            protocol: self.protocol,
            _marker: PhantomData,
        }
    }

    /// Starts an HTTP server using the specific `MakeService`.
    pub fn start<S>(self, make_service: S) -> crate::Result<()>
    where
        S: MakeServiceRef<T::Conn, Request<RequestBody>>,
        B: Backend<T::Incoming, S>,
    {
        B::start(self.protocol, self.transport.incoming(), make_service)
    }
}

impl<T, A, R> Server<DefaultTransport<T, A>, R>
where
    T: Transport,
    A: Acceptor<T::Conn>,
{
    /// Sets the instance of `Acceptor` to the server.
    ///
    /// By default, the raw acceptor is set, which returns the incoming
    /// I/Os directly.
    pub fn acceptor<A2>(self, acceptor: A2) -> Server<DefaultTransport<T, A2>, R>
    where
        A2: Acceptor<T::Conn>,
    {
        Server {
            transport: self.transport.accept(acceptor),
            protocol: self.protocol,
            _marker: PhantomData,
        }
    }

    /// Sets the time interval for sleeping on errors.
    ///
    /// If this value is set, the incoming stream sleeps for
    /// the specific period instead of terminating, and then
    /// attemps to accept again after woken up.
    ///
    /// The default value is `Some(1sec)`.
    pub fn sleep_on_errors(self, duration: Option<Duration>) -> Self {
        Self {
            transport: self.transport.sleep_on_errors(duration),
            ..self
        }
    }
}

// ==== Backend ====

#[allow(missing_debug_implementations)]
enum Never {}

/// A `Backend` indicating that the server uses the default Tokio runtime.
#[allow(missing_debug_implementations)]
pub struct Threadpool(Never);

/// A `Backend` indicating that the server uses the single-threaded Tokio runtime.
#[allow(missing_debug_implementations)]
pub struct CurrentThread(Never);

/// A trait for abstracting the process around executing the HTTP server.
pub trait Backend<I, S>: self::imp::BackendImpl<I, S> {}

mod imp {
    use super::*;

    pub trait BackendImpl<I, S> {
        fn start(protocol: Http, incoming: I, make_service: S) -> crate::Result<()>;
    }

    impl<I, S, Bd> Backend<I, S> for Threadpool
    where
        I: Stream + Send + 'static,
        I::Item: AsyncRead + AsyncWrite + Send + 'static,
        I::Error: Into<CritError>,
        S: MakeServiceRef<
                I::Item, //
                Request<RequestBody>,
                Response = Response<Bd>,
            > + Send
            + Sync
            + 'static,
        S::Error: Into<CritError>,
        S::MakeError: Into<CritError>,
        S::Future: Send + 'static,
        S::Service: Send + 'static,
        <S::Service as Service<Request<RequestBody>>>::Future: Send + 'static,
        Bd: BufStream + Send + 'static,
        Bd::Item: Send,
        Bd::Error: Into<CritError>,
    {
    }

    impl<I, S, Bd> BackendImpl<I, S> for Threadpool
    where
        I: Stream + Send + 'static,
        I::Item: AsyncRead + AsyncWrite + Send + 'static,
        I::Error: Into<CritError>,
        S: MakeServiceRef<
                I::Item, //
                Request<RequestBody>,
                Response = Response<Bd>,
            > + Send
            + Sync
            + 'static,
        S::Error: Into<CritError>,
        S::MakeError: Into<CritError>,
        S::Future: Send + 'static,
        S::Service: Send + 'static,
        <S::Service as Service<Request<RequestBody>>>::Future: Send + 'static,
        Bd: BufStream + Send + 'static,
        Bd::Item: Send,
        Bd::Error: Into<CritError>,
    {
        fn start(protocol: Http, incoming: I, make_service: S) -> crate::Result<()> {
            let protocol = protocol.with_executor(tokio::executor::DefaultExecutor::current());
            let serve = hyper::server::Builder::new(incoming, protocol) //
                .serve(LiftedMakeHttpService { make_service })
                .map_err(|e| log::error!("server error: {}", e));
            tokio::run(serve);
            Ok(())
        }
    }

    impl<I, S, Bd> Backend<I, S> for CurrentThread
    where
        I: Stream + 'static,
        I::Item: AsyncRead + AsyncWrite + Send + 'static,
        I::Error: Into<CritError>,
        S: MakeServiceRef<
                I::Item, //
                Request<RequestBody>,
                Response = Response<Bd>,
            > + 'static,
        S::Error: Into<CritError>,
        S::MakeError: Into<CritError>,
        S::Future: 'static,
        S::Service: 'static,
        <S::Service as Service<Request<RequestBody>>>::Future: 'static,
        Bd: BufStream + Send + 'static,
        Bd::Item: Send,
        Bd::Error: Into<CritError>,
    {
    }

    impl<I, S, Bd> BackendImpl<I, S> for CurrentThread
    where
        I: Stream + 'static,
        I::Item: AsyncRead + AsyncWrite + Send + 'static,
        I::Error: Into<CritError>,
        S: MakeServiceRef<
                I::Item, //
                Request<RequestBody>,
                Response = Response<Bd>,
            > + 'static,
        S::Error: Into<CritError>,
        S::MakeError: Into<CritError>,
        S::Future: 'static,
        S::Service: 'static,
        <S::Service as Service<Request<RequestBody>>>::Future: 'static,
        Bd: BufStream + Send + 'static,
        Bd::Item: Send,
        Bd::Error: Into<CritError>,
    {
        fn start(protocol: Http, incoming: I, make_service: S) -> crate::Result<()> {
            let protocol =
                protocol.with_executor(tokio::runtime::current_thread::TaskExecutor::current());
            let serve = hyper::server::Builder::new(incoming, protocol) //
                .serve(LiftedMakeHttpService { make_service })
                .map_err(|e| log::error!("server error: {}", e));

            tokio::runtime::current_thread::run(serve);
            Ok(())
        }
    }

    #[allow(missing_debug_implementations)]
    struct LiftedMakeHttpService<S> {
        make_service: S,
    }

    #[allow(clippy::type_complexity)]
    impl<'a, S, Ctx, Bd> hyper::service::MakeService<&'a Ctx> for LiftedMakeHttpService<S>
    where
        S: MakeServiceRef<Ctx, Request<RequestBody>, Response = Response<Bd>>,
        S::Error: Into<CritError>,
        S::MakeError: Into<CritError>,
        Bd: BufStream + Send + 'static,
        Bd::Item: Send,
        Bd::Error: Into<CritError>,
    {
        type ReqBody = hyper::Body;
        type ResBody = WrappedBodyStream<Bd>;
        type Error = S::Error;
        type Service = LiftedHttpService<S::Service>;
        type MakeError = S::MakeError;
        type Future = futures::future::Map<S::Future, fn(S::Service) -> Self::Service>;

        fn make_service(&mut self, ctx: &'a Ctx) -> Self::Future {
            self.make_service
                .make_service_ref(ctx)
                .map(|service| LiftedHttpService { service })
        }
    }

    #[allow(missing_debug_implementations)]
    struct LiftedHttpService<S> {
        service: S,
    }

    impl<S, Bd> hyper::service::Service for LiftedHttpService<S>
    where
        S: Service<Request<RequestBody>, Response = Response<Bd>>,
        S::Error: Into<crate::CritError>,
        Bd: BufStream + Send + 'static,
        Bd::Item: Send,
        Bd::Error: Into<CritError>,
    {
        type ReqBody = hyper::Body;
        type ResBody = WrappedBodyStream<Bd>;
        type Error = S::Error;
        type Future = LiftedHttpServiceFuture<S::Future>;

        #[inline]
        fn call(&mut self, request: Request<hyper::Body>) -> Self::Future {
            LiftedHttpServiceFuture {
                inner: self.service.call(request.map(RequestBody::from_hyp)),
            }
        }
    }

    #[allow(missing_debug_implementations)]
    struct LiftedHttpServiceFuture<Fut> {
        inner: Fut,
    }

    impl<Fut, Bd> Future for LiftedHttpServiceFuture<Fut>
    where
        Fut: Future<Item = Response<Bd>>,
        Bd: BufStream + Send + 'static,
        Bd::Item: Send,
        Bd::Error: Into<CritError>,
    {
        type Item = Response<WrappedBodyStream<Bd>>;
        type Error = Fut::Error;

        #[inline]
        fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
            self.inner
                .poll()
                .map(|x| x.map(|response| response.map(WrappedBodyStream)))
        }
    }

    #[allow(missing_debug_implementations)]
    struct WrappedBodyStream<Bd>(Bd);

    impl<Bd> hyper::body::Payload for WrappedBodyStream<Bd>
    where
        Bd: BufStream + Send + 'static,
        Bd::Item: Send,
        Bd::Error: Into<CritError>,
    {
        type Data = Bd::Item;
        type Error = Bd::Error;

        fn poll_data(&mut self) -> Poll<Option<Self::Data>, Self::Error> {
            self.0.poll_buf()
        }

        fn content_length(&self) -> Option<u64> {
            self.0.size_hint().upper()
        }
    }
}
