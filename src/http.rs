use {
    crate::error::BoxedStdError,
    crate::{
        net::{Bind, Listener},
        system::{notify, CurrentThread, DefaultRuntime, Spawn},
        tls::{NoTls, TlsConfig, TlsWrapper},
    },
    bytes::{Buf, BufMut, Bytes},
    futures::{Future, Poll, Stream},
    http::{HeaderMap, Request, Response},
    hyper::body::Payload as _Payload,
    izanami_service::{MakeService, Service},
    izanami_util::{
        buf_stream::{BufStream, SizeHint},
        http::{HasTrailers, Upgrade},
        RemoteAddr,
    },
    std::{io, marker::PhantomData},
    tokio::io::{AsyncRead, AsyncWrite},
};

pub fn server<F, S>(service_factory: F) -> Server<F>
where
    F: Fn() -> S,
    S: MakeHttpService,
{
    Server::new(service_factory)
}

/// A builder for configuring an HTTP server task.
#[derive(Debug)]
pub struct Server<F, Sig = NoSignal> {
    service_factory: F,
    shutdown_signal: Option<Sig>,
}

impl<F, S> Server<F>
where
    F: Fn() -> S,
    S: MakeHttpService,
{
    /// Creates a `Server` using the specified service factory.
    pub fn new(service_factory: F) -> Self {
        Self {
            service_factory,
            shutdown_signal: None,
        }
    }

    /// Specifies a `Future` to shutdown the server gracefully.
    pub fn with_graceful_shutdown<Sig>(self, shutdown_signal: Sig) -> Server<F, Sig>
    where
        Sig: Future<Item = ()>,
    {
        Server {
            service_factory: self.service_factory,
            shutdown_signal: Some(shutdown_signal),
        }
    }
}

impl<F, S, Sig> Server<F, Sig>
where
    F: Fn() -> S,
    S: MakeHttpService,
    Sig: Future<Item = ()>,
{
    /// Creates an `HttpTask` using the specified listener.
    pub fn bind<B>(self, bind: B) -> ServerTask<F, Sig, B>
    where
        B: Bind,
    {
        self.bind_tls(bind, NoTls::default())
    }

    /// Creates an `HttpTask` using the specified listener and TLS configuration.
    pub fn bind_tls<B, T>(self, bind: B, tls: T) -> ServerTask<F, Sig, B, T>
    where
        B: Bind,
        T: TlsConfig<B::Conn>,
    {
        ServerTask {
            service_factory: self.service_factory,
            shutdown_signal: self.shutdown_signal,
            bind,
            tls,
        }
    }
}

/// A task representing an HTTP server task spawned onto `System`.
#[derive(Debug)]
pub struct ServerTask<F, Sig, B, T = NoTls> {
    service_factory: F,
    shutdown_signal: Option<Sig>,
    bind: B,
    tls: T,
}

/// A helper macro for creating a server task.
macro_rules! spawn_inner {
    ($self:expr, $runtime:expr) => {{
        let self_ = $self;
        let runtime = $runtime;
        let result = (move || -> crate::Result<_> {
            let (tx_notify, rx_notify) = notify::pair();
            let executor = runtime.executor();

            let shutdown_signal = match self_.shutdown_signal {
                Some(sig) => futures::future::Either::A(sig.map_err(|_| ())),
                None => futures::future::Either::B(futures::future::empty::<(), ()>()),
            };

            let listener = self_.bind.bind()?;
            let tls_wrapper = self_
                .tls
                .into_wrapper(vec!["h2".into(), "http/1.1".into()])?;
            let incoming = AddrIncoming {
                listener,
                tls_wrapper,
            };
            let make_service = (self_.service_factory)();
            runtime.spawn(
                hyper::Server::builder(incoming)
                    .executor(executor)
                    .serve(hyper::service::make_service_fn(
                        move |conn: &AddrStream<_>| {
                            let remote_addr = conn.remote_addr.clone();
                            make_service
                                .make_service(&mut Context::new(&remote_addr))
                                .map(move |service| IzanamiService {
                                    service,
                                    remote_addr,
                                })
                        },
                    ))
                    .with_graceful_shutdown(shutdown_signal)
                    .then(move |result| {
                        tx_notify.send(result.map_err(Into::into));
                        Ok(())
                    }),
            );

            Ok(rx_notify)
        })();

        match result {
            Ok(rx_notify) => rx_notify,
            Err(err) => notify::Receiver::ready(Err(err)),
        }
    }};
}

impl<F, S, Sig, B, T> Spawn for ServerTask<F, Sig, B, T>
where
    F: Fn() -> S,
    S: MakeHttpService + Send + Sync + 'static,
    S::Future: Send + 'static,
    S::Service: Send + 'static,
    <S::Service as HttpService>::Future: Send + 'static,
    Sig: Future<Item = ()> + Send + 'static,
    B: Bind,
    B::Listener: Send + 'static,
    T: TlsConfig<B::Conn>,
    T::Wrapper: Send + 'static,
    T::Wrapped: Send + 'static,
{
    type Output = crate::Result<()>;

    fn spawn(self, rt: &mut DefaultRuntime) -> notify::Receiver<Self::Output> {
        spawn_inner!(self, rt)
    }
}

impl<F, S, Sig, B, T> Spawn<CurrentThread> for ServerTask<F, Sig, B, T>
where
    F: Fn() -> S,
    S: MakeHttpService + 'static,
    S::Service: 'static,
    S::Future: 'static,
    Sig: Future<Item = ()> + 'static,
    B: Bind,
    B::Listener: 'static,
    T: TlsConfig<B::Conn>,
    T::Wrapper: 'static,
    T::Wrapped: Send + 'static,
{
    type Output = crate::Result<()>;

    fn spawn(self, rt: &mut CurrentThread) -> notify::Receiver<Self::Output> {
        spawn_inner!(self, rt)
    }
}

#[doc(hidden)]
#[derive(Debug)]
pub struct NoSignal(futures::future::Empty<(), ()>);

impl Future for NoSignal {
    type Item = ();
    type Error = ();

    #[inline]
    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        self.0.poll()
    }
}

#[allow(missing_debug_implementations)]
struct IzanamiService<S> {
    service: S,
    remote_addr: RemoteAddr,
}

impl<S> hyper::service::Service for IzanamiService<S>
where
    S: HttpService,
{
    type ReqBody = hyper::Body;
    type ResBody = InnerBody<S::ResponseBody>;
    type Error = S::Error;
    type Future = LiftedHttpServiceFuture<S::Future>;

    #[inline]
    fn call(&mut self, request: Request<hyper::Body>) -> Self::Future {
        let mut request = request.map(RequestBody::from_hyp);
        request.extensions_mut().insert(self.remote_addr.clone());

        LiftedHttpServiceFuture(self.service.call(request))
    }
}

#[allow(missing_debug_implementations)]
struct LiftedHttpServiceFuture<Fut>(Fut);

impl<Fut, Bd> Future for LiftedHttpServiceFuture<Fut>
where
    Fut: Future<Item = Response<Bd>>,
    Bd: ResponseBody + Send + 'static,
{
    type Item = Response<InnerBody<Bd>>;
    type Error = Fut::Error;

    #[inline]
    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        self.0
            .poll()
            .map(|x| x.map(|response| response.map(InnerBody)))
    }
}

#[allow(missing_debug_implementations)]
struct InnerBody<Bd>(Bd);

impl<Bd> hyper::body::Payload for InnerBody<Bd>
where
    Bd: ResponseBody + Send + 'static,
{
    type Data = Bd::Item;
    type Error = crate::error::BoxedStdError;

    fn poll_data(&mut self) -> Poll<Option<Self::Data>, Self::Error> {
        self.0.poll_buf().map_err(Into::into)
    }

    fn poll_trailers(&mut self) -> Poll<Option<http::HeaderMap>, Self::Error> {
        self.0.poll_trailers().map_err(Into::into)
    }

    fn content_length(&self) -> Option<u64> {
        let hint = self.0.size_hint();
        match (hint.lower(), hint.upper()) {
            (lower, Some(upper)) if lower == upper => Some(upper),
            _ => None,
        }
    }
}

#[allow(missing_debug_implementations)]
struct AddrIncoming<T: Listener, W: TlsWrapper<T::Conn>> {
    listener: T,
    tls_wrapper: W,
}

impl<T, W> Stream for AddrIncoming<T, W>
where
    T: Listener,
    W: TlsWrapper<T::Conn>,
{
    type Item = AddrStream<W::Wrapped>;
    type Error = io::Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        self.listener.poll_incoming().map(|x| {
            x.map(|(io, remote_addr)| {
                Some(AddrStream {
                    io: self.tls_wrapper.wrap(io),
                    remote_addr,
                })
            })
        })
    }
}

#[allow(missing_debug_implementations)]
struct AddrStream<T> {
    io: T,
    remote_addr: RemoteAddr,
}

impl<T> io::Read for AddrStream<T>
where
    T: AsyncRead + AsyncWrite,
{
    #[inline]
    fn read(&mut self, dst: &mut [u8]) -> io::Result<usize> {
        self.io.read(dst)
    }
}

impl<T> io::Write for AddrStream<T>
where
    T: AsyncRead + AsyncWrite,
{
    #[inline]
    fn write(&mut self, src: &[u8]) -> io::Result<usize> {
        self.io.write(src)
    }

    #[inline]
    fn flush(&mut self) -> io::Result<()> {
        self.io.flush()
    }
}

impl<T> AsyncRead for AddrStream<T>
where
    T: AsyncRead + AsyncWrite,
{
    #[inline]
    unsafe fn prepare_uninitialized_buffer(&self, buf: &mut [u8]) -> bool {
        self.io.prepare_uninitialized_buffer(buf)
    }
}

impl<T> AsyncWrite for AddrStream<T>
where
    T: AsyncRead + AsyncWrite,
{
    #[inline]
    fn shutdown(&mut self) -> Poll<(), io::Error> {
        self.io.shutdown()
    }
}

// ==== Service ====

/// Context values passed from the server when creating the service.
///
/// Currently, there is nothing available from the value of this type.
#[derive(Debug)]
pub struct Context<'a> {
    remote_addr: &'a RemoteAddr,
    _anchor: PhantomData<std::rc::Rc<()>>,
}

impl<'a> Context<'a> {
    pub(crate) fn new(remote_addr: &'a RemoteAddr) -> Self {
        Self {
            remote_addr,
            _anchor: PhantomData,
        }
    }

    pub fn remote_addr(&self) -> &RemoteAddr {
        &*self.remote_addr
    }
}

/// An asynchronous stream of chunks that represents the HTTP request body.
#[derive(Debug)]
pub struct RequestBody(Inner);

#[derive(Debug)]
enum Inner {
    Stream(hyper::Body),
    OnUpgrade(hyper::upgrade::OnUpgrade),
}

impl RequestBody {
    pub(crate) fn from_hyp(body: hyper::Body) -> Self {
        RequestBody(Inner::Stream(body))
    }

    /// Returns whether the body is complete or not.
    pub fn is_end_stream(&self) -> bool {
        match &self.0 {
            Inner::Stream(body) => body.is_end_stream(),
            _ => true,
        }
    }

    /// Returns whether this stream has already been upgraded.
    ///
    /// If this method returns `true`, the result from `BufStream::poll_buf`
    /// or `HasTrailers::poll_trailers` becomes an error.
    pub fn is_upgraded(&self) -> bool {
        match self.0 {
            Inner::OnUpgrade(..) => true,
            _ => false,
        }
    }

    /// Returns a length of the total bytes, if possible.
    pub fn content_length(&self) -> Option<u64> {
        match &self.0 {
            Inner::Stream(body) => body.content_length(),
            _ => None,
        }
    }
}

impl BufStream for RequestBody {
    type Item = io::Cursor<Bytes>;
    type Error = BoxedStdError;

    fn poll_buf(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        match &mut self.0 {
            Inner::Stream(body) => body
                .poll_data()
                .map(|x| x.map(|opt| opt.map(|chunk| io::Cursor::new(chunk.into_bytes()))))
                .map_err(Into::into),
            Inner::OnUpgrade(..) => Err(already_upgraded()),
        }
    }

    fn size_hint(&self) -> SizeHint {
        match &self.0 {
            Inner::Stream(body) => {
                let mut hint = SizeHint::new();
                if let Some(len) = body.content_length() {
                    hint.set_upper(len);
                    hint.set_lower(len);
                }
                hint
            }
            Inner::OnUpgrade(..) => SizeHint::new(),
        }
    }
}

impl HasTrailers for RequestBody {
    type TrailersError = BoxedStdError;

    fn poll_trailers(&mut self) -> Poll<Option<HeaderMap>, Self::TrailersError> {
        match &mut self.0 {
            Inner::Stream(body) => body.poll_trailers().map_err(Into::into),
            Inner::OnUpgrade(..) => Err(already_upgraded()),
        }
    }
}

impl Upgrade for RequestBody {
    type Upgraded = Upgraded;
    type Error = BoxedStdError;

    fn poll_upgrade(&mut self) -> Poll<Self::Upgraded, Self::Error> {
        loop {
            self.0 = match &mut self.0 {
                Inner::Stream(body) => {
                    let body = std::mem::replace(body, hyper::Body::empty());
                    Inner::OnUpgrade(body.on_upgrade())
                }
                Inner::OnUpgrade(on_upgrade) => {
                    return on_upgrade
                        .poll()
                        .map(|x| x.map(Upgraded))
                        .map_err(Into::into);
                }
            };
        }
    }
}

#[derive(Debug)]
pub struct Upgraded(hyper::upgrade::Upgraded);

impl Upgraded {
    #[inline]
    pub fn downcast<T>(self) -> Result<(T, Bytes), Self>
    where
        T: AsyncRead + AsyncWrite + 'static,
    {
        self.0
            .downcast::<T>()
            .map(|parts| (parts.io, parts.read_buf))
            .map_err(Upgraded)
    }
}

impl io::Read for Upgraded {
    #[inline]
    fn read(&mut self, dst: &mut [u8]) -> io::Result<usize> {
        self.0.read(dst)
    }
}

impl io::Write for Upgraded {
    #[inline]
    fn write(&mut self, src: &[u8]) -> io::Result<usize> {
        self.0.write(src)
    }

    #[inline]
    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}

impl AsyncRead for Upgraded {
    #[inline]
    unsafe fn prepare_uninitialized_buffer(&self, buf: &mut [u8]) -> bool {
        self.0.prepare_uninitialized_buffer(buf)
    }

    #[inline]
    fn read_buf<B: BufMut>(&mut self, buf: &mut B) -> Poll<usize, io::Error> {
        self.0.read_buf(buf)
    }
}

impl AsyncWrite for Upgraded {
    #[inline]
    fn write_buf<B: Buf>(&mut self, buf: &mut B) -> Poll<usize, io::Error> {
        self.0.write_buf(buf)
    }

    #[inline]
    fn shutdown(&mut self) -> Poll<(), io::Error> {
        self.0.shutdown()
    }
}

pub trait MakeHttpService {
    type ResponseBody: ResponseBody + Send + 'static;
    type Error: Into<BoxedStdError>;
    type Service: HttpService<ResponseBody = Self::ResponseBody, Error = Self::Error>;
    type MakeError: Into<BoxedStdError>;
    type Future: Future<Item = Self::Service, Error = Self::MakeError>;

    fn make_service(&self, cx: &mut Context<'_>) -> Self::Future;
}

impl<S, Bd, SvcErr, MkErr, Svc, Fut> MakeHttpService for S
where
    S: for<'cx, 'srv> MakeService<
        &'cx mut Context<'srv>, //
        Request<RequestBody>,
        Response = Response<Bd>,
        Error = SvcErr,
        Service = Svc,
        MakeError = MkErr,
        Future = Fut,
    >,
    SvcErr: Into<BoxedStdError>,
    MkErr: Into<BoxedStdError>,
    Svc: Service<Request<RequestBody>, Response = Response<Bd>, Error = SvcErr>,
    Fut: Future<Item = Svc, Error = MkErr>,
    Bd: ResponseBody + Send + 'static,
{
    type ResponseBody = Bd;
    type Error = SvcErr;
    type Service = Svc;
    type MakeError = MkErr;
    type Future = Fut;

    fn make_service(&self, cx: &mut Context<'_>) -> Self::Future {
        MakeService::make_service(self, cx)
    }
}

pub trait HttpService {
    type ResponseBody: ResponseBody + Send + 'static;
    type Error: Into<BoxedStdError>;
    type Future: Future<Item = Response<Self::ResponseBody>, Error = Self::Error>;

    fn call(&mut self, request: Request<RequestBody>) -> Self::Future;
}

impl<S, Bd> HttpService for S
where
    S: Service<Request<RequestBody>, Response = Response<Bd>>,
    S::Error: Into<BoxedStdError>,
    Bd: ResponseBody + Send + 'static,
{
    type ResponseBody = Bd;
    type Error = S::Error;
    type Future = S::Future;

    fn call(&mut self, request: Request<RequestBody>) -> Self::Future {
        Service::call(self, request)
    }
}

pub trait ResponseBody {
    type Item: Buf + Send;
    type Error: Into<BoxedStdError>;
    type TrailersError: Into<BoxedStdError>;

    fn poll_buf(&mut self) -> Poll<Option<Self::Item>, Self::Error>;

    fn poll_trailers(&mut self) -> Poll<Option<HeaderMap>, Self::TrailersError>;

    fn size_hint(&self) -> SizeHint;
}

impl<Bd> ResponseBody for Bd
where
    Bd: BufStream + HasTrailers,
    Bd::Item: Send,
    Bd::Error: Into<BoxedStdError>,
    Bd::TrailersError: Into<BoxedStdError>,
{
    type Item = Bd::Item;
    type Error = Bd::Error;
    type TrailersError = Bd::TrailersError;

    #[inline]
    fn poll_buf(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        BufStream::poll_buf(self)
    }

    #[inline]
    fn poll_trailers(&mut self) -> Poll<Option<HeaderMap>, Self::TrailersError> {
        HasTrailers::poll_trailers(self)
    }

    #[inline]
    fn size_hint(&self) -> SizeHint {
        BufStream::size_hint(self)
    }
}

fn already_upgraded() -> BoxedStdError {
    failure::format_err!("the request body has already been upgraded")
        .compat()
        .into()
}
