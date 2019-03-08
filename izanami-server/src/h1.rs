//! HTTP/1 connection using hyper as backend.

use {
    crate::{server::Connection, BoxedStdError},
    bytes::{Buf, BufMut, Bytes, IntoBuf},
    futures::{Async, Future, Poll},
    http::{HeaderMap, Request, Response},
    hyper::{body::Payload as _Payload, server::conn::Http},
    izanami_http::{body::HttpBody, HttpService, Upgrade},
    izanami_util::*,
    std::io,
    tokio::{
        io::{AsyncRead, AsyncWrite},
        sync::oneshot,
    },
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

#[doc(hidden)]
#[allow(missing_debug_implementations)]
pub struct DummyService(());

impl<T> izanami_service::Service<T> for DummyService {
    type Response = Response<String>;
    type Error = hyper::Error;
    type Future = futures::future::Empty<Self::Response, Self::Error>;

    fn poll_ready(&mut self) -> Poll<(), Self::Error> {
        unreachable!("DummyService never used")
    }

    fn call(&mut self, _: T) -> Self::Future {
        unreachable!("DummyService never used")
    }
}

/// A builder for configuration of `H1Connection`.
#[derive(Debug)]
pub struct Builder<I> {
    stream: I,
    protocol: Http<DummyExecutor>,
}

impl<I> Builder<I>
where
    I: AsyncRead + AsyncWrite + 'static,
{
    /// Creates a new `Builder` with the specified `stream`.
    pub fn new(stream: I) -> Self {
        let mut protocol = Http::new() //
            .with_executor(DummyExecutor);
        protocol.http1_only(true);
        Self { stream, protocol }
    }

    /// Sets whether the connection should support half-closures.
    ///
    /// This method corresponds to [`http1_half_close`].
    ///
    /// The default value is `true`.
    ///
    /// [`http1_half_close`]: https://docs.rs/hyper/0.12/hyper/server/conn/struct.Http.html#method.http1_half_close
    pub fn half_close(mut self, enabled: bool) -> Self {
        self.protocol.http1_half_close(enabled);
        self
    }

    /// Sets whether the connection should try to use vectored writers.
    ///
    /// This method corresponds to [`http1_writev`].
    ///
    /// The default value is `true`.
    ///
    /// [`http1_writev`]: https://docs.rs/hyper/0.12/hyper/server/conn/struct.Http.html#method.http1_writev
    pub fn writev(mut self, enabled: bool) -> Self {
        self.protocol.http1_writev(enabled);
        self
    }

    /// Sets whether to enable HTTP keep-alive.
    ///
    /// The default value is `true`.
    pub fn keep_alive(mut self, enabled: bool) -> Self {
        self.protocol.keep_alive(enabled);
        self
    }

    /// Sets the maximum buffer size for this connection.
    pub fn max_buf_size(mut self, amt: usize) -> Self {
        self.protocol.max_buf_size(amt);
        self
    }

    /// Consumes itself and create an `H1Connection` with the specified `Service`.
    pub fn finish<S>(self, service: S) -> H1Connection<I, S>
    where
        S: HttpService<RequestBody<I>> + 'static,
        S::ResponseBody: Send + 'static,
        <S::ResponseBody as HttpBody>::Data: Send,
        <S::ResponseBody as HttpBody>::Error: Into<BoxedStdError>,
        S::Error: Into<BoxedStdError>,
    {
        H1Connection {
            conn: Some(self.protocol.serve_connection(
                self.stream,
                InnerService {
                    inner: service,
                    tx_upgraded: None,
                },
            )),
        }
    }
}

/// A `Connection` that serves an HTTP/1 connection using hyper.
///
/// HTTP/2 is disabled.
#[allow(missing_debug_implementations)]
pub struct H1Connection<I, S>
where
    I: AsyncRead + AsyncWrite + 'static,
    S: HttpService<RequestBody<I>>,
    S::ResponseBody: Send + 'static,
    <S::ResponseBody as HttpBody>::Data: Send,
    <S::ResponseBody as HttpBody>::Error: Into<BoxedStdError>,
    S::Error: Into<BoxedStdError>,
{
    conn: Option<hyper::server::conn::Connection<I, InnerService<I, S>, DummyExecutor>>,
}

impl<I> H1Connection<I, DummyService>
where
    I: AsyncRead + AsyncWrite + 'static,
{
    /// Start building using the specified I/O.
    pub fn build(stream: I) -> Builder<I> {
        Builder::new(stream)
    }
}

impl<I, S, Bd> Connection for H1Connection<I, S>
where
    I: AsyncRead + AsyncWrite + 'static,
    S: HttpService<RequestBody<I>, ResponseBody = Bd> + 'static,
    S::Error: Into<BoxedStdError>,
    Bd: HttpBody + Send + 'static,
    Bd::Data: Send,
    Bd::Error: Into<BoxedStdError>,
{
    type Error = BoxedStdError;

    fn poll_complete(&mut self) -> Poll<(), Self::Error> {
        if let Some(ref mut conn) = self.conn {
            futures::try_ready!(conn.poll_without_shutdown());
        }

        if let Some(conn) = self.conn.take() {
            let hyper::server::conn::Parts {
                io,
                read_buf,
                service: InnerService { tx_upgraded, .. },
                ..
            } = conn.into_parts();

            if let Some(tx) = tx_upgraded {
                let _ = tx.send(Upgraded {
                    io,
                    read_buf: Some(read_buf),
                });
            }
        }

        Ok(Async::Ready(()))
    }

    fn graceful_shutdown(&mut self) {
        if let Some(ref mut conn) = self.conn {
            conn.graceful_shutdown();
        }
    }
}

#[allow(missing_debug_implementations)]
struct InnerService<I, S> {
    inner: S,
    tx_upgraded: Option<oneshot::Sender<Upgraded<I>>>,
}

impl<I, S, Bd> hyper::service::Service for InnerService<I, S>
where
    I: AsyncRead + AsyncWrite,
    S: HttpService<RequestBody<I>, ResponseBody = Bd>,
    S::Error: Into<BoxedStdError>,
    Bd: HttpBody + Send + 'static,
    Bd::Data: Send,
    Bd::Error: Into<BoxedStdError>,
{
    type ReqBody = hyper::Body;
    type ResBody = InnerBody<Bd>;
    type Error = BoxedStdError;
    type Future = InnerServiceFuture<S::Future>;

    #[inline]
    fn call(&mut self, request: Request<Self::ReqBody>) -> Self::Future {
        let (tx_upgraded, rx_upgraded) = oneshot::channel();
        self.tx_upgraded = Some(tx_upgraded);
        let request = request.map(|body| RequestBody { body, rx_upgraded });
        InnerServiceFuture {
            inner: self.inner.respond(request),
        }
    }
}

#[allow(missing_debug_implementations)]
struct InnerServiceFuture<Fut> {
    inner: Fut,
}

impl<Fut, Bd> Future for InnerServiceFuture<Fut>
where
    Fut: Future<Item = Response<Bd>>,
    Fut::Error: Into<BoxedStdError>,
    Bd: HttpBody + Send + 'static,
    Bd::Data: Send,
    Bd::Error: Into<BoxedStdError>,
{
    type Item = Response<InnerBody<Bd>>;
    type Error = BoxedStdError;

    #[inline]
    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        self.inner
            .poll()
            .map_async(|response| response.map(|inner| InnerBody { inner }))
            .map_err(Into::into)
    }
}

#[allow(missing_debug_implementations)]
struct InnerBody<Bd> {
    inner: Bd,
}

impl<Bd> hyper::body::Payload for InnerBody<Bd>
where
    Bd: HttpBody + Send + 'static,
    Bd::Data: Send,
    Bd::Error: Into<BoxedStdError>,
{
    type Data = Bd::Data;
    type Error = BoxedStdError;

    #[inline]
    fn poll_data(&mut self) -> Poll<Option<Self::Data>, Self::Error> {
        HttpBody::poll_data(&mut self.inner).map_err(Into::into)
    }

    #[inline]
    fn poll_trailers(&mut self) -> Poll<Option<http::HeaderMap>, Self::Error> {
        HttpBody::poll_trailers(&mut self.inner).map_err(Into::into)
    }

    #[inline]
    fn content_length(&self) -> Option<u64> {
        None
    }
}

/// The message body of an incoming HTTP request.
#[derive(Debug)]
pub struct RequestBody<I> {
    body: hyper::Body,
    rx_upgraded: oneshot::Receiver<Upgraded<I>>,
}

impl<I> RequestBody<I> {
    /// Returns whether the body is complete or not.
    pub fn is_end_stream(&self) -> bool {
        self.body.is_end_stream()
    }

    /// Returns a length of the total bytes, if possible.
    pub fn content_length(&self) -> Option<u64> {
        self.body.content_length()
    }
}

impl<I> BufStream for RequestBody<I> {
    type Item = Data;
    type Error = BoxedStdError;

    fn poll_buf(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        self.body
            .poll_data()
            .map_async_opt(Data)
            .map_err(Into::into)
    }

    fn size_hint(&self) -> SizeHint {
        SizeHint::new()
    }
}

impl<I> HttpBody for RequestBody<I> {
    type Data = Data;
    type Error = BoxedStdError;

    fn poll_data(&mut self) -> Poll<Option<Self::Data>, Self::Error> {
        BufStream::poll_buf(self)
    }

    fn size_hint(&self) -> tokio_buf::SizeHint {
        BufStream::size_hint(self)
    }

    fn poll_trailers(&mut self) -> Poll<Option<HeaderMap>, Self::Error> {
        self.body.poll_trailers().map_err(Into::into)
    }

    fn is_end_stream(&self) -> bool {
        self.body.is_end_stream()
    }

    fn content_length(&self) -> Option<u64> {
        self.body.content_length()
    }
}

impl<I> Upgrade for RequestBody<I>
where
    I: AsyncRead + AsyncWrite,
{
    type Upgraded = Upgraded<I>;
    type Error = BoxedStdError;
    type Future = OnUpgrade<I>;

    fn on_upgrade(self) -> Self::Future {
        OnUpgrade(self.rx_upgraded)
    }
}

#[derive(Debug)]
pub struct Data(hyper::body::Chunk);

impl Data {
    pub fn into_bytes(self) -> Bytes {
        self.0.into_bytes()
    }
}

impl AsRef<[u8]> for Data {
    fn as_ref(&self) -> &[u8] {
        self.0.bytes()
    }
}

impl Buf for Data {
    fn remaining(&self) -> usize {
        self.0.remaining()
    }

    fn bytes(&self) -> &[u8] {
        self.0.bytes()
    }

    fn advance(&mut self, cnt: usize) {
        self.0.advance(cnt);
    }
}

#[derive(Debug)]
pub struct OnUpgrade<I>(oneshot::Receiver<Upgraded<I>>);

impl<I> Future for OnUpgrade<I> {
    type Item = Upgraded<I>;
    type Error = BoxedStdError;

    #[inline]
    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        self.0
            .poll()
            .map_err(|_| failure::format_err!("recv error").compat().into())
    }
}

// FIXME: impl AsyncRead, AsyncWrite
#[derive(Debug)]
pub struct Upgraded<I> {
    io: I,
    read_buf: Option<Bytes>,
}

impl<I> Upgraded<I>
where
    I: AsyncRead + AsyncWrite,
{
    pub fn into_parts(self) -> (I, Option<Bytes>) {
        (self.io, self.read_buf)
    }
}

impl<I> io::Read for Upgraded<I>
where
    I: AsyncRead + AsyncWrite,
{
    fn read(&mut self, dst: &mut [u8]) -> io::Result<usize> {
        if let Some(buf) = self.read_buf.take() {
            if buf.len() > 0 {
                let mut pre_reader = buf.into_buf().reader();
                let read_cnt = pre_reader.read(dst)?;

                let mut new_pre = pre_reader.into_inner().into_inner();
                new_pre.advance(read_cnt);

                if new_pre.len() > 0 {
                    self.read_buf = Some(new_pre);
                }

                return Ok(read_cnt);
            }
        }
        self.io.read(dst)
    }
}

impl<I> io::Write for Upgraded<I>
where
    I: AsyncRead + AsyncWrite,
{
    fn write(&mut self, src: &[u8]) -> io::Result<usize> {
        self.io.write(src)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.io.flush()
    }
}

impl<I> AsyncRead for Upgraded<I>
where
    I: AsyncRead + AsyncWrite,
{
    unsafe fn prepare_uninitialized_buffer(&self, buf: &mut [u8]) -> bool {
        self.io.prepare_uninitialized_buffer(buf)
    }

    fn read_buf<B: BufMut>(&mut self, buf: &mut B) -> Poll<usize, io::Error> {
        if let Some(bs) = self.read_buf.take() {
            let pre_len = bs.len();
            if pre_len > 0 {
                let cnt = std::cmp::min(buf.remaining_mut(), pre_len);
                let pre_buf = bs.into_buf();
                let mut xfer = Buf::take(pre_buf, cnt);
                buf.put(&mut xfer);

                let mut new_pre = xfer.into_inner().into_inner();
                new_pre.advance(cnt);

                if new_pre.len() > 0 {
                    self.read_buf = Some(new_pre);
                }

                return Ok(Async::Ready(cnt));
            }
        }
        self.io.read_buf(buf)
    }
}

impl<I> AsyncWrite for Upgraded<I>
where
    I: AsyncRead + AsyncWrite,
{
    fn shutdown(&mut self) -> Poll<(), io::Error> {
        AsyncWrite::shutdown(&mut self.io)
    }

    fn write_buf<B: Buf>(&mut self, buf: &mut B) -> Poll<usize, io::Error> {
        AsyncWrite::write_buf(&mut self.io, buf)
    }
}

/// Type alias representing the HTTP request passed to the services.
pub type HttpRequest<I> = http::Request<RequestBody<I>>;
