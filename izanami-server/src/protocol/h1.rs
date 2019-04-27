//! HTTP/1 connection.

use {
    crate::BoxedStdError,
    crate::{server::Connection, upgrade::HttpUpgrade},
    bytes::{Buf, Bytes},
    futures::{try_ready, Async, Future, Poll},
    http::{HeaderMap, Request, Response},
    hyper::{
        body::Payload as _Payload,
        server::conn::{Connection as HyperConnection, Http},
    },
    izanami_http::{body::HttpBody, HttpService},
    izanami_util::{MapAsyncOptExt, RewindIo},
    tokio::{
        io::{AsyncRead, AsyncWrite},
        sync::oneshot,
    },
    tokio_buf::SizeHint,
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

/// Type alias of `http::Request<T>` passed by `H1Connection`.
pub type H1Request = http::Request<RequestBody>;

/// A builder for configuration H1 connections.
#[derive(Debug, Clone)]
pub struct H1 {
    protocol: Http<DummyExecutor>,
}

impl H1 {
    /// Creates a new `H1` with the default configuration.
    pub fn new() -> Self {
        Self {
            protocol: {
                let mut protocol = Http::new() //
                    .with_executor(DummyExecutor);
                protocol.http1_only(true);
                protocol
            },
        }
    }

    /// Sets whether the connection should support half-closures.
    ///
    /// This method corresponds to [`http1_half_close`].
    ///
    /// The default value is `true`.
    ///
    /// [`http1_half_close`]: https://docs.rs/hyper/0.12/hyper/server/conn/struct.Http.html#method.http1_half_close
    pub fn half_close(&mut self, enabled: bool) -> &mut Self {
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
    pub fn writev(&mut self, enabled: bool) -> &mut Self {
        self.protocol.http1_writev(enabled);
        self
    }

    /// Sets whether to enable HTTP keep-alive.
    ///
    /// The default value is `true`.
    pub fn keep_alive(&mut self, enabled: bool) -> &mut Self {
        self.protocol.keep_alive(enabled);
        self
    }

    /// Sets the maximum buffer size for this connection.
    pub fn max_buf_size(&mut self, amt: usize) -> &mut Self {
        self.protocol.max_buf_size(amt);
        self
    }

    /// Build an `H1Connection` using the current configuration.
    pub fn serve<I, S>(&self, stream: I, service: S) -> H1Connection<I, S>
    where
        I: AsyncRead + AsyncWrite + 'static,
        S: HttpService<RequestBody> + 'static,
        S::ResponseBody: HttpUpgrade<RewindIo<I>> + Send + 'static,
        <S::ResponseBody as HttpBody>::Data: Send,
        <S::ResponseBody as HttpBody>::Error: Into<BoxedStdError>,
        <S::ResponseBody as HttpUpgrade<RewindIo<I>>>::Error: Into<BoxedStdError>,
        S::Error: Into<BoxedStdError>,
    {
        let service = InnerService {
            service,
            rx_body: None,
        };
        let conn = self.protocol.serve_connection(stream, service);
        H1Connection {
            state: State::InFlight(conn),
        }
    }
}

impl Default for H1 {
    fn default() -> Self {
        Self::new()
    }
}

/// A `Connection` that serves an HTTP/1 connection.
///
/// It uses the low level server API of `hyper`, with the modified
/// implementation around HTTP/1.1 upgrade mechanism.
#[allow(missing_debug_implementations)]
pub struct H1Connection<I, S>
where
    I: AsyncRead + AsyncWrite + 'static,
    S: HttpService<RequestBody>,
    S::ResponseBody: HttpUpgrade<RewindIo<I>> + Send + 'static,
    <S::ResponseBody as HttpBody>::Data: Send,
    <S::ResponseBody as HttpBody>::Error: Into<BoxedStdError>,
    <S::ResponseBody as HttpUpgrade<RewindIo<I>>>::Error: Into<BoxedStdError>,
    S::Error: Into<BoxedStdError>,
{
    state: State<I, S>,
}

#[allow(missing_debug_implementations)]
enum State<I, S>
where
    I: AsyncRead + AsyncWrite + 'static,
    S: HttpService<RequestBody>,
    S::ResponseBody: HttpUpgrade<RewindIo<I>> + Send + 'static,
    <S::ResponseBody as HttpBody>::Data: Send,
    <S::ResponseBody as HttpBody>::Error: Into<BoxedStdError>,
    <S::ResponseBody as HttpUpgrade<RewindIo<I>>>::Error: Into<BoxedStdError>,
    S::Error: Into<BoxedStdError>,
{
    InFlight(HyperConnection<I, InnerService<S>, DummyExecutor>),
    WillUpgrade {
        rewind: RewindIo<I>,
        rx_body: oneshot::Receiver<S::ResponseBody>,
    },
    Upgraded(<S::ResponseBody as HttpUpgrade<RewindIo<I>>>::Upgraded),
    Shutdown(RewindIo<I>),
    Closed,
}

impl<I, S, Bd> H1Connection<I, S>
where
    I: AsyncRead + AsyncWrite + 'static,
    S: HttpService<RequestBody, ResponseBody = Bd> + 'static,
    S::Error: Into<BoxedStdError>,
    Bd: HttpBody + HttpUpgrade<RewindIo<I>> + Send + 'static,
    Bd::Data: Send,
    <Bd as HttpBody>::Error: Into<BoxedStdError>,
    <Bd as HttpUpgrade<RewindIo<I>>>::Error: Into<BoxedStdError>,
{
    fn poll_complete_inner(&mut self) -> Poll<(), BoxedStdError> {
        loop {
            let mut body = None;
            match self.state {
                State::InFlight(ref mut conn) => {
                    // run HTTP dispatcher without calling `AsyncRead::shutdown`.
                    try_ready!(conn.poll_without_shutdown());
                }
                State::WillUpgrade {
                    ref mut rx_body, ..
                } => {
                    // acquire the upgrade context.
                    body = Some(try_ready!(rx_body.poll().map_err(
                        |_| failure::format_err!("error during receiving upgrade context")
                    )));
                }
                State::Upgraded(ref mut upgraded) => {
                    return upgraded.poll_close().map_err(Into::into);
                }
                State::Shutdown(ref mut stream) => {
                    // shutdown the underlying I/O manually.
                    return stream.shutdown().map_err(Into::into);
                }
                State::Closed => return Ok(Async::Ready(())),
            }

            self.state = match std::mem::replace(&mut self.state, State::Closed) {
                State::InFlight(conn) => {
                    // deconstruct hyper's Connection into the underlying parts.
                    let hyper::server::conn::Parts {
                        io,
                        read_buf,
                        service: InnerService { rx_body, .. },
                        ..
                    } = conn.into_parts();

                    let rewind = RewindIo::new_buffered(io, read_buf);

                    if let Some((rx_body, _reqbd)) = rx_body {
                        State::WillUpgrade { rewind, rx_body }
                    } else {
                        State::Shutdown(rewind)
                    }
                }

                State::WillUpgrade { rewind, .. } => {
                    let body = body.expect("the response body must be available");
                    match body.upgrade(rewind) {
                        Ok(upgraded) => State::Upgraded(upgraded),
                        Err(rewind) => State::Shutdown(rewind),
                    }
                }

                State::Upgraded { .. } | State::Shutdown { .. } | State::Closed => unreachable!(),
            }
        }
    }
}

impl<I, S, Bd> Connection for H1Connection<I, S>
where
    I: AsyncRead + AsyncWrite + 'static,
    S: HttpService<RequestBody, ResponseBody = Bd> + 'static,
    S::Error: Into<BoxedStdError>,
    Bd: HttpBody + HttpUpgrade<RewindIo<I>> + Send + 'static,
    Bd::Data: Send,
    <Bd as HttpBody>::Error: Into<BoxedStdError>,
    <Bd as HttpUpgrade<RewindIo<I>>>::Error: Into<BoxedStdError>,
{
    type Error = BoxedStdError;

    fn poll_close(&mut self) -> Poll<(), Self::Error> {
        let res = match self.poll_complete_inner() {
            Ok(Async::NotReady) => return Ok(Async::NotReady),
            res => res,
        };
        self.state = State::Closed;
        res
    }

    fn graceful_shutdown(&mut self) {
        match self.state {
            State::InFlight(ref mut conn) => conn.graceful_shutdown(),
            State::Upgraded(ref mut upgraded) => upgraded.graceful_shutdown(),
            _ => (),
        }
    }
}

#[allow(missing_debug_implementations)]
struct InnerService<S>
where
    S: HttpService<RequestBody>,
{
    service: S,
    rx_body: Option<(oneshot::Receiver<S::ResponseBody>, hyper::Body)>,
}

impl<S, Bd> hyper::service::Service for InnerService<S>
where
    S: HttpService<RequestBody, ResponseBody = Bd>,
    S::Error: Into<BoxedStdError>,
    Bd: HttpBody + Send + 'static,
    Bd::Data: Send,
    Bd::Error: Into<BoxedStdError>,
{
    type ReqBody = hyper::Body;
    type ResBody = InnerBody<Bd>;
    type Error = BoxedStdError;
    type Future = InnerServiceFuture<S>;

    #[inline]
    fn poll_ready(&mut self) -> Poll<(), Self::Error> {
        self.service.poll_ready().map_err(Into::into)
    }

    #[inline]
    fn call(&mut self, mut request: Request<Self::ReqBody>) -> Self::Future {
        // clears the previous upgrade context.
        drop(self.rx_body.take());

        let is_connect = request.method() == http::Method::CONNECT;
        let will_upgrade = is_connect
            || request
                .headers()
                .get(http::header::CONNECTION)
                .map_or(false, |h| match h.as_bytes() {
                    b"upgrade" | b"Upgrade" => {
                        request.headers().contains_key(http::header::UPGRADE)
                    }
                    _ => false,
                });

        let mut tx_body = None;
        if will_upgrade {
            let (tx, rx) = oneshot::channel();
            let body = std::mem::replace(request.body_mut(), hyper::Body::empty());
            tx_body = Some(tx);
            self.rx_body = Some((rx, body));
        }

        InnerServiceFuture {
            respond: self.service.respond(request.map(RequestBody)),
            is_connect,
            tx_body,
        }
    }
}

#[allow(missing_debug_implementations)]
struct InnerServiceFuture<S: HttpService<RequestBody>> {
    respond: S::Respond,
    is_connect: bool,
    tx_body: Option<oneshot::Sender<S::ResponseBody>>,
}

impl<S, Bd> Future for InnerServiceFuture<S>
where
    S: HttpService<RequestBody, ResponseBody = Bd>,
    S::Error: Into<BoxedStdError>,
    Bd: HttpBody + Send + 'static,
    Bd::Data: Send,
    Bd::Error: Into<BoxedStdError>,
{
    type Item = Response<InnerBody<Bd>>;
    type Error = BoxedStdError;

    #[inline]
    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let response = try_ready!(self.respond.poll().map_err(Into::into));
        let tx_body = self.tx_body.take();

        let upgraded = response.status() == http::StatusCode::SWITCHING_PROTOCOLS
            || (self.is_connect && response.status().is_success());

        let response = {
            let (parts, body) = response.into_parts();

            let body_inner = match (tx_body, upgraded) {
                (Some(tx), true) => {
                    log::trace!("send the response body for protocol upgrade.");
                    tx.send(body).err()
                }
                _ => Some(body),
            };

            Response::from_parts(parts, InnerBody(body_inner))
        };

        Ok(Async::Ready(response))
    }
}

#[allow(missing_debug_implementations)]
struct InnerBody<Bd>(Option<Bd>);

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
        match self.0 {
            Some(ref mut body) => HttpBody::poll_data(body).map_err(Into::into),
            None => Ok(Async::Ready(None)),
        }
    }

    #[inline]
    fn poll_trailers(&mut self) -> Poll<Option<http::HeaderMap>, Self::Error> {
        match self.0 {
            Some(ref mut body) => HttpBody::poll_trailers(body).map_err(Into::into),
            None => Ok(Async::Ready(None)),
        }
    }

    #[inline]
    fn is_end_stream(&self) -> bool {
        match self.0 {
            Some(ref body) => HttpBody::is_end_stream(body),
            None => true,
        }
    }

    #[inline]
    fn content_length(&self) -> Option<u64> {
        match self.0 {
            Some(ref body) => HttpBody::content_length(body),
            None => None,
        }
    }
}

/// The message body of an incoming HTTP request.
#[derive(Debug)]
pub struct RequestBody(hyper::Body);

impl RequestBody {
    /// Returns whether the body is complete or not.
    pub fn is_end_stream(&self) -> bool {
        self.0.is_end_stream()
    }

    /// Returns a length of the total bytes, if possible.
    pub fn content_length(&self) -> Option<u64> {
        self.0.content_length()
    }
}

impl HttpBody for RequestBody {
    type Data = Data;
    type Error = BoxedStdError;

    fn poll_data(&mut self) -> Poll<Option<Self::Data>, Self::Error> {
        self.0.poll_data().map_async_opt(Data).map_err(Into::into)
    }

    fn size_hint(&self) -> tokio_buf::SizeHint {
        SizeHint::new()
    }

    fn poll_trailers(&mut self) -> Poll<Option<HeaderMap>, Self::Error> {
        self.0.poll_trailers().map_err(Into::into)
    }

    fn is_end_stream(&self) -> bool {
        self.0.is_end_stream()
    }

    fn content_length(&self) -> Option<u64> {
        self.0.content_length()
    }
}

/// A chunk of bytes received from the client.
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
