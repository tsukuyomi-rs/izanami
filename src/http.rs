use {
    crate::error::BoxedStdError,
    crate::{
        net::{Bind, Listener},
        server::{NewTask, NewTaskConfig},
        tls::{Acceptor, NoTls},
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
    tokio::{
        io::{AsyncRead, AsyncWrite},
        sync::oneshot,
    },
};

/// A builder for configuring an HTTP server task.
#[derive(Debug)]
pub struct Http<B: Bind> {
    bind: B,
    protocol: hyper::server::conn::Http,
}

impl<B: Bind> Http<B> {
    /// Creates a `Http` to configure a task of serving HTTP services.
    pub fn bind(bind: B) -> Self {
        Self {
            bind,
            protocol: hyper::server::conn::Http::new(),
        }
    }

    /// Spawn an HTTP server task onto the server.
    pub fn serve<F, S>(self, service_factory: F) -> NewHttpTask<F, B>
    where
        F: Fn() -> S,
        S: MakeHttpService,
    {
        self.serve_with(NoTls::default(), service_factory)
    }

    /// Spawn an HTTP server task onto the server with the specified TLS acceptor.
    pub fn serve_with<F, A, S>(self, acceptor: A, service_factory: F) -> NewHttpTask<F, B, A>
    where
        F: Fn() -> S,
        S: MakeHttpService,
        A: Acceptor<B::Conn>,
    {
        let Self { bind, protocol, .. } = self;
        NewHttpTask {
            service_factory,
            bind,
            acceptor,
            protocol,
        }
    }
}

#[derive(Debug)]
pub struct NewHttpTask<F, B, A = NoTls> {
    service_factory: F,
    bind: B,
    acceptor: A,
    protocol: hyper::server::conn::Http,
}

impl<F, B, A, S, Rt> NewTask<Rt> for NewHttpTask<F, B, A>
where
    F: Fn() -> S,
    S: MakeHttpService,
    B: Bind,
    A: Acceptor<B::Conn>,
    Rt: SpawnHttp<S, B::Listener, A>,
{
    fn new_task(&self, rt: &mut Rt, config: NewTaskConfig) -> crate::Result<()> {
        let cx = SpawnHttpContext {
            make_service: (self.service_factory)(),
            listener: self.bind.bind()?,
            acceptor: self.acceptor.clone(),
            protocol: self.protocol.clone(),
            shutdown_signal: config.shutdown_signal,
        };
        rt.spawn_http(cx);
        Ok(())
    }
}

#[doc(hidden)]
#[allow(missing_debug_implementations)]
pub struct SpawnHttpContext<S, T, A = crate::tls::NoTls> {
    make_service: S,
    listener: T,
    acceptor: A,
    protocol: hyper::server::conn::Http,
    shutdown_signal: oneshot::Receiver<()>,
}

/// A trait that represents spawning of HTTP server tasks.
pub trait SpawnHttp<S, T, A = crate::tls::NoTls>
where
    S: MakeHttpService,
    T: Listener,
    A: Acceptor<T::Conn>,
{
    fn spawn_http(&mut self, cx: SpawnHttpContext<S, T, A>);
}

/// A helper macro for creating a server task.
macro_rules! serve {
    ($cx:expr, $executor:expr) => {{
        let cx = $cx;
        let executor = $executor;

        let incoming = AddrIncoming {
            listener: cx.listener,
            acceptor: cx.acceptor,
        };
        let make_service = cx.make_service;

        hyper::server::Builder::new(incoming, cx.protocol)
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
            .with_graceful_shutdown(cx.shutdown_signal)
            .map_err(|e| log::error!("server error: {}", e))
    }};
}

impl<S, T, A> SpawnHttp<S, T, A> for tokio::runtime::Runtime
where
    S: MakeHttpService + Send + Sync + 'static,
    S::Future: Send + 'static,
    S::Service: Send + 'static,
    <S::Service as HttpService>::Future: Send + 'static,
    T: Listener + Send + 'static,
    A: Acceptor<T::Conn> + Send + 'static,
    A::Accepted: Send + 'static,
{
    fn spawn_http(&mut self, cx: SpawnHttpContext<S, T, A>) {
        self.spawn(serve!(cx, self.executor()));
    }
}

impl<S, T, A> SpawnHttp<S, T, A> for tokio::runtime::current_thread::Runtime
where
    S: MakeHttpService + 'static,
    S::Service: 'static,
    S::Future: 'static,
    T: Listener + 'static,
    A: Acceptor<T::Conn> + 'static,
    A::Accepted: Send + 'static,
{
    fn spawn_http(&mut self, cx: SpawnHttpContext<S, T, A>) {
        let executor = tokio::runtime::current_thread::TaskExecutor::current();
        self.spawn(serve!(cx, executor));
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
struct AddrIncoming<T: Listener, A: Acceptor<T::Conn>> {
    listener: T,
    acceptor: A,
}

impl<T, A> Stream for AddrIncoming<T, A>
where
    T: Listener,
    A: Acceptor<T::Conn>,
{
    type Item = AddrStream<A::Accepted>;
    type Error = io::Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        self.listener.poll_incoming().map(|x| {
            x.map(|(io, remote_addr)| {
                Some(AddrStream {
                    io: self.acceptor.accept(io),
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
