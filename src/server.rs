use {
    crate::{
        io::{Acceptor, Incoming, Listener},
        CritError, RequestBody,
    },
    futures::{Future, Poll},
    http::{Request, Response},
    hyper::server::conn::Http,
    izanami_http::buf_stream::BufStream,
    izanami_service::{MakeServiceRef, Service},
    std::{marker::PhantomData, net::SocketAddr, time::Duration},
};

/// A simple HTTP server that wraps the `hyper`'s server implementation.
#[derive(Debug)]
pub struct Server<
    L = SocketAddr, //
    A = (),
    R = tokio::runtime::Runtime,
> {
    listener: L,
    acceptor: A,
    sleep_on_errors: Option<Duration>,
    protocol: Http,
    _marker: PhantomData<R>,
}

impl Server {
    /// Creates an HTTP server using a TCP transport with the address `"127.0.0.1:4000"`.
    pub fn build() -> Self {
        Server::bind(([127, 0, 0, 1], 4000).into())
    }
}

impl<L> Server<L>
where
    L: Listener,
{
    /// Create a new `Server` with the specified `NewService` and default configuration.
    pub fn bind(listener: L) -> Self {
        Self {
            listener,
            acceptor: (),
            sleep_on_errors: Some(Duration::from_secs(1)),
            protocol: Http::new(),
            _marker: PhantomData,
        }
    }
}

impl<L, A, R> Server<L, A, R>
where
    L: Listener,
    A: Acceptor<L::Conn>,
{
    /// Sets the instance of `Acceptor` to the server.
    ///
    /// By default, the raw acceptor is set, which returns the incoming
    /// I/Os directly.
    pub fn acceptor<A2>(self, acceptor: A2) -> Server<L, A2, R>
    where
        A2: Acceptor<L::Conn>,
    {
        Server {
            listener: self.listener,
            acceptor,
            sleep_on_errors: self.sleep_on_errors,
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
            sleep_on_errors: duration,
            ..self
        }
    }

    /// Returns a reference to the HTTP-level configuration.
    pub fn protocol(&mut self) -> &mut Http {
        &mut self.protocol
    }

    /// Switches the runtime to be used to [`current_thread::Runtime`].
    ///
    /// [`current_thread::Runtime`]: https://docs.rs/tokio/0.1/tokio/runtime/current_thread/struct.Runtime.html
    pub fn current_thread(self) -> Server<L, A, tokio::runtime::current_thread::Runtime> {
        Server {
            listener: self.listener,
            acceptor: self.acceptor,
            sleep_on_errors: self.sleep_on_errors,
            protocol: self.protocol,
            _marker: PhantomData,
        }
    }
}

impl<T, A> Server<T, A, tokio::runtime::Runtime>
where
    T: Listener,
    T::Incoming: Send + 'static,
    A: Acceptor<T::Conn> + Send + 'static,
    A::Accepted: Send + 'static,
{
    /// Starts an HTTP server using the supplied value of `MakeService`.
    pub fn start<S, Bd>(self, make_service: S) -> crate::Result<()>
    where
        S: MakeServiceRef<A::Accepted, Request<RequestBody>, Response = Response<Bd>>
            + Send
            + Sync
            + 'static,
        S::Error: Into<crate::CritError>,
        S::MakeError: Into<crate::CritError>,
        S::Future: Send + 'static,
        S::Service: Send + 'static,
        <S::Service as Service<Request<RequestBody>>>::Future: Send + 'static,
        Bd: BufStream + Send + 'static,
        Bd::Item: Send,
        Bd::Error: Into<CritError>,
    {
        let Self {
            listener,
            acceptor,
            sleep_on_errors,
            protocol,
            ..
        } = self;

        let incoming = Incoming::new(
            listener
                .listen()
                .map_err(|err| failure::Error::from_boxed_compat(err.into()))?,
            acceptor,
            sleep_on_errors,
        );

        let protocol = protocol.with_executor(tokio::executor::DefaultExecutor::current());

        let serve = hyper::server::Builder::new(incoming, protocol) //
            .serve(LiftedMakeHttpService { make_service })
            .map_err(|e| log::error!("server error: {}", e));

        tokio::run(serve);
        Ok(())
    }
}

impl<T, A> Server<T, A, tokio::runtime::current_thread::Runtime>
where
    T: Listener,
    T::Incoming: 'static,
    A: Acceptor<T::Conn> + 'static,
    A::Accepted: Send + 'static,
{
    /// Starts an HTTP server using the supplied value of `MakeService`, with a current-thread runtime.
    pub fn start<S, Bd>(self, make_service: S) -> crate::Result<()>
    where
        S: MakeServiceRef<A::Accepted, Request<RequestBody>, Response = Response<Bd>> + 'static,
        S::Error: Into<crate::CritError>,
        S::MakeError: Into<crate::CritError>,
        S::Future: 'static,
        S::Service: 'static,
        <S::Service as Service<Request<RequestBody>>>::Future: 'static,
        Bd: BufStream + Send + 'static,
        Bd::Item: Send,
        Bd::Error: Into<CritError>,
    {
        let Self {
            listener,
            acceptor,
            sleep_on_errors,
            protocol,
            ..
        } = self;

        let incoming = Incoming::new(
            listener
                .listen()
                .map_err(|err| failure::Error::from_boxed_compat(err.into()))?,
            acceptor,
            sleep_on_errors,
        );

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
            inner: self.service.call(request.map(RequestBody)),
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
