use {
    crate::{drain::Watch, server::Connection, BoxedStdError},
    bytes::Bytes,
    futures::{Future, Poll},
    http::{HeaderMap, Request, Response},
    hyper::{body::Payload as _Payload, server::conn::Http},
    izanami_http::{
        body::{BodyTrailers, ContentLength, HttpBody},
        HttpService,
    },
    izanami_util::*,
    std::io,
    tokio::io::{AsyncRead, AsyncWrite},
    tokio_buf::{BufStream, SizeHint},
};

#[derive(Debug, Clone)]
struct DummyExecutor;

impl<F> futures::future::Executor<F> for DummyExecutor
where
    F: Future<Item = (), Error = ()> + 'static,
{
    fn execute(&self, _: F) -> Result<(), futures::future::ExecuteError<F>> {
        unreachable!()
    }
}

#[derive(Debug)]
pub struct Builder<I> {
    stream: I,
    protocol: Http<DummyExecutor>,
}

impl<I> Builder<I>
where
    I: AsyncRead + AsyncWrite,
{
    pub fn new(stream: I) -> Self {
        let mut protocol = Http::new() //
            .with_executor(DummyExecutor);
        protocol.http1_only(true);
        Self { stream, protocol }
    }

    pub fn half_close(mut self, enabled: bool) -> Self {
        self.protocol.http1_half_close(enabled);
        self
    }

    pub fn writev(mut self, enabled: bool) -> Self {
        self.protocol.http1_writev(enabled);
        self
    }

    pub fn keep_alive(mut self, enabled: bool) -> Self {
        self.protocol.keep_alive(enabled);
        self
    }

    pub fn max_buf_size(mut self, amt: usize) -> Self {
        self.protocol.max_buf_size(amt);
        self
    }

    pub fn pipeline_flush(mut self, enabled: bool) -> Self {
        self.protocol.pipeline_flush(enabled);
        self
    }

    /// Specifies the `Service` to serve incoming HTTP requests.
    pub fn serve<S>(self, service: S) -> H1Connection<I, S>
    where
        S: HttpService<RequestBody>,
    {
        H1Connection {
            stream: self.stream,
            service,
            protocol: self.protocol,
        }
    }
}

/// A `Connection` that serves an HTTP/1 connection using hyper.
///
/// Only HTTP/1 is available.
#[derive(Debug)]
pub struct H1Connection<I, S> {
    stream: I,
    service: S,
    protocol: Http<DummyExecutor>,
}

impl<I> H1Connection<I, ()>
where
    I: AsyncRead + AsyncWrite,
{
    /// Creates a `Builder` of this type using the specified I/O.
    pub fn builder(stream: I) -> Builder<I> {
        Builder::new(stream)
    }
}

impl<I, S> Connection for H1Connection<I, S>
where
    I: AsyncRead + AsyncWrite + 'static,
    S: HttpService<RequestBody> + 'static,
    S::ResponseBody: Send + 'static,
    <S::ResponseBody as BufStream>::Item: Send,
    <S::ResponseBody as BufStream>::Error: Into<BoxedStdError>,
    <S::ResponseBody as BodyTrailers>::TrailersError: Into<BoxedStdError>,
    S::Error: Into<BoxedStdError>,
{
    type Future = H1Task<I, S>;

    fn into_future(self, watch: Watch) -> Self::Future {
        H1Task {
            conn: self
                .protocol
                .serve_connection(self.stream, InnerService(self.service)),
            watch,
            state: H1TaskState::Running,
        }
    }
}

#[doc(hidden)]
#[allow(missing_debug_implementations)]
pub struct H1Task<I, S>
where
    I: AsyncRead + AsyncWrite + 'static,
    S: HttpService<RequestBody> + 'static,
    S::ResponseBody: Send + 'static,
    <S::ResponseBody as BufStream>::Item: Send,
    <S::ResponseBody as BufStream>::Error: Into<BoxedStdError>,
    <S::ResponseBody as BodyTrailers>::TrailersError: Into<BoxedStdError>,
    S::Error: Into<BoxedStdError>,
{
    conn: hyper::server::conn::Connection<I, InnerService<S>, DummyExecutor>,
    watch: Watch,
    state: H1TaskState,
}

enum H1TaskState {
    Running,
    Drained,
}

impl<I, S> Future for H1Task<I, S>
where
    I: AsyncRead + AsyncWrite + 'static,
    S: HttpService<RequestBody> + 'static,
    S::ResponseBody: Send + 'static,
    <S::ResponseBody as BufStream>::Item: Send,
    <S::ResponseBody as BufStream>::Error: Into<BoxedStdError>,
    <S::ResponseBody as BodyTrailers>::TrailersError: Into<BoxedStdError>,
    S::Error: Into<BoxedStdError>,
{
    type Item = ();
    type Error = ();

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        loop {
            if let H1TaskState::Running = self.state {
                if self.watch.poll_signal() {
                    self.state = H1TaskState::Drained;
                    self.conn.graceful_shutdown();
                    continue;
                }
            }
            return self
                .conn
                .poll()
                .map_err(|e| log::error!("connection error: {}", e));
        }
    }
}

#[doc(hidden)]
#[allow(missing_debug_implementations)]
pub struct InnerService<S>(S);

impl<S> hyper::service::Service for InnerService<S>
where
    S: HttpService<RequestBody> + 'static,
    S::ResponseBody: Send + 'static,
    <S::ResponseBody as BufStream>::Item: Send,
    <S::ResponseBody as BufStream>::Error: Into<BoxedStdError>,
    <S::ResponseBody as BodyTrailers>::TrailersError: Into<BoxedStdError>,
    S::Error: Into<BoxedStdError>,
{
    type ReqBody = hyper::Body;
    type ResBody = InnerBody<S>;
    type Error = BoxedStdError;
    type Future = InnerServiceFuture<S>;

    #[inline]
    fn call(&mut self, request: Request<hyper::Body>) -> Self::Future {
        let request = request.map(RequestBody::from_hyp);
        InnerServiceFuture {
            inner: self.0.respond(request),
        }
    }
}

#[doc(hidden)]
#[allow(missing_debug_implementations)]
pub struct InnerServiceFuture<S: HttpService<RequestBody>> {
    inner: S::Future,
}

impl<S> Future for InnerServiceFuture<S>
where
    S: HttpService<RequestBody>,
    S::ResponseBody: Send + 'static,
    <S::ResponseBody as BufStream>::Item: Send,
    <S::ResponseBody as BufStream>::Error: Into<BoxedStdError>,
    <S::ResponseBody as BodyTrailers>::TrailersError: Into<BoxedStdError>,
    S::Error: Into<BoxedStdError>,
{
    type Item = Response<InnerBody<S>>;
    type Error = BoxedStdError;

    #[inline]
    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        self.inner
            .poll()
            .map_async(|response| response.map(|inner| InnerBody { inner }))
            .map_err(Into::into)
    }
}

#[doc(hidden)]
#[allow(missing_debug_implementations)]
pub struct InnerBody<S: HttpService<RequestBody>> {
    inner: S::ResponseBody,
}

impl<S> hyper::body::Payload for InnerBody<S>
where
    S: HttpService<RequestBody> + 'static,
    S::ResponseBody: Send + 'static,
    <S::ResponseBody as BufStream>::Item: Send,
    <S::ResponseBody as BufStream>::Error: Into<BoxedStdError>,
    <S::ResponseBody as BodyTrailers>::TrailersError: Into<BoxedStdError>,
{
    type Data = <S::ResponseBody as BufStream>::Item;
    type Error = BoxedStdError;

    #[inline]
    fn poll_data(&mut self) -> Poll<Option<Self::Data>, Self::Error> {
        BufStream::poll_buf(&mut self.inner).map_err(Into::into)
    }

    #[inline]
    fn poll_trailers(&mut self) -> Poll<Option<http::HeaderMap>, Self::Error> {
        BodyTrailers::poll_trailers(&mut self.inner).map_err(Into::into)
    }

    #[inline]
    fn content_length(&self) -> Option<u64> {
        match HttpBody::content_length(&self.inner) {
            ContentLength::Sized(len) => Some(len),
            ContentLength::Chunked => None,
        }
    }
}

/// The message body of an incoming HTTP request.
#[derive(Debug)]
pub struct RequestBody(hyper::Body);

impl RequestBody {
    pub(crate) fn from_hyp(body: hyper::Body) -> Self {
        RequestBody(body)
    }

    /// Returns whether the body is complete or not.
    pub fn is_end_stream(&self) -> bool {
        self.0.is_end_stream()
    }

    /// Returns a length of the total bytes, if possible.
    pub fn content_length(&self) -> Option<u64> {
        self.0.content_length()
    }
}

impl BufStream for RequestBody {
    type Item = io::Cursor<Bytes>;
    type Error = BoxedStdError;

    fn poll_buf(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        self.0
            .poll_data()
            .map(|x| x.map(|opt| opt.map(|chunk| io::Cursor::new(chunk.into_bytes()))))
            .map_err(Into::into)
    }

    fn size_hint(&self) -> SizeHint {
        let mut hint = SizeHint::new();
        if let Some(len) = self.0.content_length() {
            hint.set_upper(len);
            hint.set_lower(len);
        }
        hint
    }
}

impl BodyTrailers for RequestBody {
    type TrailersError = BoxedStdError;

    fn poll_trailers(&mut self) -> Poll<Option<HeaderMap>, Self::TrailersError> {
        self.0.poll_trailers().map_err(Into::into)
    }
}

impl HttpBody for RequestBody {
    fn is_end_stream(&self) -> bool {
        self.0.is_end_stream()
    }

    fn content_length(&self) -> ContentLength {
        match self.0.content_length() {
            Some(len) => ContentLength::Sized(len),
            None => ContentLength::Chunked,
        }
    }
}

/// Type alias representing the HTTP request passed to the services.
pub type HttpRequest = http::Request<RequestBody>;
