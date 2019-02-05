//! A lightweight implementation of HTTP server based on Hyper.

pub mod conn;

use {
    self::conn::{Acceptor, Listener, MakeListener, WithAcceptor},
    crate::CritError,
    futures::{Future, Poll},
    http::{HeaderMap, Request, Response},
    hyper::{body::Payload as _Payload, server::conn::Http},
    izanami_service::{MakeService, Service},
    izanami_util::{
        buf_stream::{BufStream, SizeHint},
        http::{HasTrailers, Upgrade},
        RemoteAddr,
    },
    std::{fmt, marker::PhantomData},
};

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
    type TrailersError = hyper::Error;

    fn poll_trailers(&mut self) -> Poll<Option<HeaderMap>, Self::TrailersError> {
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

// ==== MakeServiceContext ====

/// A type representing the context information that can be used from the inside
/// of `MakeService::make_service`.
#[derive(Debug)]
pub struct MakeServiceContext<'a, T: Listener> {
    conn: &'a T::Conn,
    _anchor: PhantomData<std::rc::Rc<()>>,
}

impl<'a, T> MakeServiceContext<'a, T>
where
    T: Listener,
{
    /// Returns a reference to the instance of a connection to a peer.
    pub fn conn(&self) -> &T::Conn {
        &*self.conn
    }
}

impl<'a, T> std::ops::Deref for MakeServiceContext<'a, T>
where
    T: Listener,
{
    type Target = T::Conn;

    fn deref(&self) -> &Self::Target {
        self.conn()
    }
}

// ==== Server ====

/// A simple HTTP server that wraps the `hyper`'s server implementation.
pub struct Server<T, B = Threadpool> {
    listener: T,
    protocol: Http,
    _marker: PhantomData<B>,
}

impl<T, B> fmt::Debug for Server<T, B>
where
    T: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Server")
            .field("listener", &self.listener)
            .field("protocol", &self.protocol)
            .finish()
    }
}

impl Server<()> {
    /// Creates an HTTP server from the specified listener.
    pub fn bind<T>(listener: T) -> Result<Server<T::Listener>, T::Error>
    where
        T: MakeListener,
    {
        let listener = listener.make_listener()?;
        Ok(Server {
            listener,
            protocol: Http::new(),
            _marker: PhantomData,
        })
    }
}

impl<T, B> Server<T, B>
where
    T: Listener,
{
    /// Returns a reference to the inner listener.
    pub fn listener(&self) -> &T {
        &self.listener
    }

    /// Returns a mutable reference to the inner listener.
    pub fn listener_mut(&mut self) -> &mut T {
        &mut self.listener
    }

    /// Returns a reference to the HTTP-level configuration.
    pub fn protocol(&self) -> &Http {
        &self.protocol
    }

    /// Returns a mutable reference to the HTTP-level configuration.
    pub fn protocol_mut(&mut self) -> &mut Http {
        &mut self.protocol
    }

    /// Sets the instance of `Acceptor` to the server.
    pub fn acceptor<A>(self, acceptor: A) -> Server<WithAcceptor<T, A>, B>
    where
        A: Acceptor<T::Conn>,
    {
        Server {
            listener: WithAcceptor::new(self.listener, acceptor),
            protocol: self.protocol,
            _marker: PhantomData,
        }
    }

    pub(crate) fn backend<B2>(self) -> Server<T, B2> {
        Server {
            listener: self.listener,
            protocol: self.protocol,
            _marker: PhantomData,
        }
    }

    /// Switches the backend to `CurrentThread`.
    pub fn current_thread(self) -> Server<T, CurrentThread> {
        self.backend()
    }

    /// Start this HTTP server with the specified service factory.
    pub fn start<S>(self, make_service: S) -> crate::Result<()>
    where
        S: for<'a> MakeService<MakeServiceContext<'a, T>, Request<RequestBody>>,
        B: Backend<T, S>,
    {
        B::start(
            self.protocol, //
            self.listener.incoming(),
            make_service,
            None,
        )
    }

    /// Start this HTTP server with the specified service factory and shutdown signal.
    pub fn start_with_graceful_shutdown<S, Sig>(
        self,
        make_service: S,
        signal: Sig,
    ) -> crate::Result<()>
    where
        S: for<'a> MakeService<MakeServiceContext<'a, T>, Request<RequestBody>>,
        Sig: Future<Item = ()>,
        B: Backend<T, S, Sig>,
    {
        B::start(
            self.protocol,
            self.listener.incoming(),
            make_service,
            Some(signal),
        )
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
pub trait Backend<T, S, Sig = futures::future::Empty<(), ()>>:
    self::imp::BackendImpl<T, S, Sig>
where
    T: Listener,
    S: for<'a> MakeService<MakeServiceContext<'a, T>, Request<RequestBody>>,
    Sig: Future<Item = ()>,
{
}

pub(crate) mod imp {
    use super::*;

    pub trait BackendImpl<T, S, Sig>
    where
        T: Listener,
        S: for<'a> MakeService<MakeServiceContext<'a, T>, Request<RequestBody>>,
        Sig: Future<Item = ()>,
    {
        fn start(
            protocol: Http, //
            incoming: T::Incoming,
            make_service: S,
            shutdown_signal: Option<Sig>,
        ) -> crate::Result<()>;
    }

    impl<T, S, Bd, SvcErr, Svc, MkErr, Fut, Sig> Backend<T, S, Sig> for Threadpool
    where
        T: Listener + 'static,
        T::Conn: Send + 'static,
        T::Incoming: Send + 'static,
        S: for<'a> MakeService<
                MakeServiceContext<'a, T>, //
                Request<RequestBody>,
                Response = Response<Bd>,
                Error = SvcErr,
                Service = Svc,
                MakeError = MkErr,
                Future = Fut,
            > + Send
            + Sync
            + 'static,
        SvcErr: Into<CritError>,
        MkErr: Into<CritError>,
        Fut: Future<Item = Svc, Error = MkErr> + Send + 'static,
        Svc: Service<
                Request<RequestBody>, //
                Response = Response<Bd>,
                Error = SvcErr,
            > + Send
            + 'static,
        Svc::Future: Send + 'static,
        Bd: BufStream + Send + 'static,
        Bd::Item: Send,
        Bd::Error: Into<CritError>,
        Sig: Future<Item = ()> + Send + 'static,
    {
    }

    impl<T, S, Bd, SvcErr, Svc, MkErr, Fut, Sig> BackendImpl<T, S, Sig> for Threadpool
    where
        T: Listener + 'static,
        T::Conn: Send + 'static,
        T::Incoming: Send + 'static,
        S: for<'a> MakeService<
                MakeServiceContext<'a, T>, //
                Request<RequestBody>,
                Response = Response<Bd>,
                Error = SvcErr,
                Service = Svc,
                MakeError = MkErr,
                Future = Fut,
            > + Send
            + Sync
            + 'static,
        SvcErr: Into<CritError>,
        MkErr: Into<CritError>,
        Fut: Future<Item = Svc, Error = MkErr> + Send + 'static,
        Svc: Service<
                Request<RequestBody>, //
                Response = Response<Bd>,
                Error = SvcErr,
            > + Send
            + 'static,
        Svc::Future: Send + 'static,
        Bd: BufStream + Send + 'static,
        Bd::Item: Send,
        Bd::Error: Into<CritError>,
        Sig: Future<Item = ()> + Send + 'static,
    {
        fn start(
            protocol: Http,
            incoming: T::Incoming,
            make_service: S,
            shutdown_signal: Option<Sig>,
        ) -> crate::Result<()> {
            let protocol = protocol.with_executor(tokio::executor::DefaultExecutor::current());
            let serve = hyper::server::Builder::new(incoming, protocol) //
                .serve(LiftedMakeHttpService::new(make_service));

            if let Some(shutdown_signal) = shutdown_signal {
                tokio::run(
                    serve
                        .with_graceful_shutdown(shutdown_signal)
                        .map_err(|e| log::error!("server error: {}", e)),
                );
            } else {
                tokio::run(
                    serve //
                        .map_err(|e| log::error!("server error: {}", e)),
                );
            }

            Ok(())
        }
    }

    impl<T, S, Bd, SvcErr, Svc, MkErr, Fut, Sig> Backend<T, S, Sig> for CurrentThread
    where
        T: Listener + 'static,
        T::Conn: Send + 'static,
        T::Incoming: 'static,
        S: for<'a> MakeService<
                MakeServiceContext<'a, T>, //
                Request<RequestBody>,
                Response = Response<Bd>,
                Error = SvcErr,
                Service = Svc,
                MakeError = MkErr,
                Future = Fut,
            > + 'static,
        SvcErr: Into<CritError>,
        MkErr: Into<CritError>,
        Svc: Service<
                Request<RequestBody>, //
                Response = Response<Bd>,
                Error = SvcErr,
            > + 'static,
        Svc::Future: 'static,
        Fut: Future<Item = Svc, Error = MkErr> + 'static,
        Bd: BufStream + Send + 'static,
        Bd::Item: Send,
        Bd::Error: Into<CritError>,
        Sig: Future<Item = ()> + 'static,
    {
    }

    impl<T, S, Bd, SvcErr, Svc, MkErr, Fut, Sig> BackendImpl<T, S, Sig> for CurrentThread
    where
        T: Listener + 'static,
        T::Conn: Send + 'static,
        T::Incoming: 'static,
        S: for<'a> MakeService<
                MakeServiceContext<'a, T>, //
                Request<RequestBody>,
                Response = Response<Bd>,
                Error = SvcErr,
                Service = Svc,
                MakeError = MkErr,
                Future = Fut,
            > + 'static,
        SvcErr: Into<CritError>,
        MkErr: Into<CritError>,
        Svc: Service<
                Request<RequestBody>, //
                Response = Response<Bd>,
                Error = SvcErr,
            > + 'static,
        Svc::Future: 'static,
        Fut: Future<Item = Svc, Error = MkErr> + 'static,
        Bd: BufStream + Send + 'static,
        Bd::Item: Send,
        Bd::Error: Into<CritError>,
        Sig: Future<Item = ()> + 'static,
    {
        fn start(
            protocol: Http,
            incoming: T::Incoming,
            make_service: S,
            shutdown_signal: Option<Sig>,
        ) -> crate::Result<()> {
            let protocol =
                protocol.with_executor(tokio::runtime::current_thread::TaskExecutor::current());

            let serve = hyper::server::Builder::new(incoming, protocol) //
                .serve(LiftedMakeHttpService::new(make_service));

            if let Some(shutdown_signal) = shutdown_signal {
                tokio::runtime::current_thread::run(
                    serve
                        .with_graceful_shutdown(shutdown_signal)
                        .map_err(|e| log::error!("server error: {}", e)),
                );
            } else {
                tokio::runtime::current_thread::run(
                    serve //
                        .map_err(|e| log::error!("server error: {}", e)),
                );
            }

            Ok(())
        }
    }

    #[allow(missing_debug_implementations)]
    pub(crate) struct LiftedMakeHttpService<T, S> {
        make_service: S,
        _marker: PhantomData<fn(&T)>,
    }

    impl<T, S> LiftedMakeHttpService<T, S> {
        pub(crate) fn new(make_service: S) -> Self {
            Self {
                make_service,
                _marker: PhantomData,
            }
        }
    }

    #[allow(clippy::type_complexity)]
    impl<'a, T, S, Bd> hyper::service::MakeService<&'a T::Conn> for LiftedMakeHttpService<T, S>
    where
        T: Listener,
        S: MakeService<
            MakeServiceContext<'a, T>, //
            Request<RequestBody>,
            Response = Response<Bd>,
        >,
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
        type Future = LiftedMakeHttpServiceFuture<S::Future>;

        fn make_service(&mut self, conn: &'a T::Conn) -> Self::Future {
            let remote_addr = T::remote_addr(conn);
            LiftedMakeHttpServiceFuture {
                inner: self.make_service.make_service(MakeServiceContext {
                    conn,
                    _anchor: PhantomData,
                }),
                remote_addr: Some(remote_addr),
            }
        }
    }

    #[allow(missing_debug_implementations)]
    pub(crate) struct LiftedMakeHttpServiceFuture<Fut> {
        inner: Fut,
        remote_addr: Option<RemoteAddr>,
    }

    impl<Fut> Future for LiftedMakeHttpServiceFuture<Fut>
    where
        Fut: Future,
    {
        type Item = LiftedHttpService<Fut::Item>;
        type Error = Fut::Error;

        #[inline]
        fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
            Ok(LiftedHttpService {
                service: futures::try_ready!(self.inner.poll()),
                remote_addr: self
                    .remote_addr
                    .take()
                    .expect("the future has already been polled."),
            }
            .into())
        }
    }

    #[allow(missing_debug_implementations)]
    pub(crate) struct LiftedHttpService<S> {
        service: S,
        remote_addr: RemoteAddr,
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
            let mut request = request.map(RequestBody::from_hyp);
            request.extensions_mut().insert(self.remote_addr.clone());

            LiftedHttpServiceFuture {
                inner: self.service.call(request),
            }
        }
    }

    #[allow(missing_debug_implementations)]
    pub(crate) struct LiftedHttpServiceFuture<Fut> {
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
    pub(crate) struct WrappedBodyStream<Bd>(Bd);

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
