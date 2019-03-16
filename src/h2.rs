//! HTTP/2 connection.

// TODOs:
// * flow control
// * protocol upgrade
// * shutdown background streams

use {
    crate::BoxedStdError, //
    bytes::{Buf, BufMut, Bytes, BytesMut},
    futures::{stream::FuturesUnordered, try_ready, Async, Future, Poll, Stream},
    http::{Request, Response},
    httpdate::HttpDate,
    izanami_http::{Connection, HttpBody, HttpService},
    std::time::SystemTime,
    tokio::io::{AsyncRead, AsyncWrite},
};

pub type H2Request = Request<RequestBody>;

/// A builder for creating an `H2Connection`.
#[derive(Debug)]
pub struct Builder<I> {
    stream: I,
    protocol: h2::server::Builder,
}

/// A `Connection` that serves an HTTP/2 connection.
#[allow(missing_debug_implementations)]
pub struct H2Connection<I, S>
where
    S: HttpService<RequestBody>,
{
    state: State<I, S>,
    service: S,
    backgrounds: FuturesUnordered<Background<S>>,
}

#[allow(missing_debug_implementations)]
enum State<I, S: HttpService<RequestBody>> {
    Handshake(h2::server::Handshake<I, SendBuf<<S::ResponseBody as HttpBody>::Data>>),
    Running(h2::server::Connection<I, SendBuf<<S::ResponseBody as HttpBody>::Data>>),
    Closed,
}

#[allow(missing_debug_implementations)]
struct Background<S: HttpService<RequestBody>> {
    state: BackgroundState<S>,
}

#[allow(missing_debug_implementations)]
enum BackgroundState<S: HttpService<RequestBody>> {
    Responding(Respond<S>),
    Sending(SendBody<S::ResponseBody>),
}

#[allow(missing_debug_implementations)]
struct Respond<S: HttpService<RequestBody>> {
    future: S::Future,
    reply: h2::server::SendResponse<SendBuf<<S::ResponseBody as HttpBody>::Data>>,
}

#[allow(missing_debug_implementations)]
struct SendBody<B: HttpBody> {
    body: B,
    tx_stream: h2::SendStream<SendBuf<B::Data>>,
    end_of_data: bool,
}

#[allow(missing_debug_implementations)]
enum SendBuf<T> {
    Data(T),
    Eos,
}

/// An `HttpBody` to received the data from client.
#[derive(Debug)]
pub struct RequestBody {
    recv: h2::RecvStream,
}

/// A chunk of bytes received from the client.
#[derive(Debug)]
pub struct Data(Bytes);

// ===== impl H2Connection =====

impl<I, S> H2Connection<I, S>
where
    I: AsyncRead + AsyncWrite,
    S: HttpService<RequestBody>,
    S::Error: Into<BoxedStdError>,
    <S::ResponseBody as HttpBody>::Data: 'static,
    <S::ResponseBody as HttpBody>::Error: Into<BoxedStdError>,
{
    /// Creates a new `H2Connection` with the default configuration.
    pub fn new(stream: I, service: S) -> Self {
        Builder::new(stream).build(service)
    }
}

impl<I, S> H2Connection<I, S>
where
    I: AsyncRead + AsyncWrite,
    S: HttpService<RequestBody>,
    S::Error: Into<BoxedStdError>,
    <S::ResponseBody as HttpBody>::Data: 'static,
    <S::ResponseBody as HttpBody>::Error: Into<BoxedStdError>,
{
    fn poll_foreground2(&mut self) -> Poll<(), BoxedStdError> {
        loop {
            self.state = match self.state {
                State::Handshake(ref mut handshake) => {
                    let conn = try_ready!(handshake.poll());
                    State::Running(conn)
                }
                State::Running(ref mut conn) => {
                    if let Async::NotReady = self.service.poll_ready().map_err(Into::into)? {
                        try_ready!(conn.poll_close());
                        return Ok(Async::Ready(()));
                    }

                    if let Some((req, reply)) = try_ready!(conn.poll()) {
                        let req = req.map(|recv| RequestBody { recv });
                        let future = self.service.respond(req);
                        self.backgrounds.push(Background {
                            state: BackgroundState::Responding(Respond { future, reply }),
                        });
                        continue;
                    } else {
                        return Ok(Async::Ready(()));
                    }
                }
                State::Closed => return Ok(Async::Ready(())),
            };
        }
    }

    fn poll_foreground(&mut self) -> Poll<(), BoxedStdError> {
        match self.poll_foreground2() {
            Ok(Async::Ready(())) => {
                self.state = State::Closed;
                Ok(Async::Ready(()))
            }
            Ok(Async::NotReady) => Ok(Async::NotReady),
            Err(e) => {
                self.state = State::Closed;
                Err(e)
            }
        }
    }

    fn poll_background(&mut self) -> Poll<(), BoxedStdError> {
        loop {
            match self.backgrounds.poll() {
                Ok(Async::Ready(Some(()))) => continue,
                Ok(Async::Ready(None)) => return Ok(Async::Ready(())),
                Ok(Async::NotReady) => return Ok(Async::NotReady),
                Err(err) => {
                    log::trace!("background error: {}", err);
                    return Ok(Async::Ready(()));
                }
            }
        }
    }
}

impl<I, S> Connection for H2Connection<I, S>
where
    I: AsyncRead + AsyncWrite,
    S: HttpService<RequestBody>,
    S::Error: Into<BoxedStdError>,
    <S::ResponseBody as HttpBody>::Data: 'static,
    <S::ResponseBody as HttpBody>::Error: Into<BoxedStdError>,
{
    type Error = BoxedStdError;

    fn poll_close(&mut self) -> Poll<(), Self::Error> {
        let status = self.poll_foreground()?;
        try_ready!(self.poll_background());
        Ok(status)
    }

    fn graceful_shutdown(&mut self) {
        match self.state {
            State::Handshake(..) => (),
            State::Running(ref mut conn) => conn.graceful_shutdown(),
            State::Closed => (),
        }
    }
}

// ===== impl Builder =====

impl<I> Builder<I>
where
    I: AsyncRead + AsyncWrite,
{
    /// Creates a new `Builder` with the specified transport.
    pub fn new(stream: I) -> Self {
        Self {
            stream,
            protocol: h2::server::Builder::new(),
        }
    }

    /// Returns a mutable reference to the protocol level configuration.
    pub fn protocol(&mut self) -> &mut h2::server::Builder {
        &mut self.protocol
    }

    /// Builds a `H2Connection` with the specified service.
    pub fn build<S>(self, service: S) -> H2Connection<I, S>
    where
        S: HttpService<RequestBody>,
        S::Error: Into<BoxedStdError>,
        <S::ResponseBody as HttpBody>::Data: 'static,
        <S::ResponseBody as HttpBody>::Error: Into<BoxedStdError>,
    {
        let handshake = self.protocol.handshake(self.stream);
        H2Connection {
            state: State::Handshake(handshake),
            service,
            backgrounds: FuturesUnordered::new(),
        }
    }
}

// ===== impl RequestBody =====

impl HttpBody for RequestBody {
    type Data = Data;
    type Error = BoxedStdError;

    fn poll_data(&mut self) -> Poll<Option<Self::Data>, Self::Error> {
        let res = try_ready!(self.recv.poll());
        Ok(Async::Ready(res.map(|data| {
            self.recv
                .release_capacity()
                .release_capacity(data.len())
                .expect("the released capacity should be valid");
            Data(data)
        })))
    }

    fn poll_trailers(&mut self) -> Poll<Option<http::HeaderMap>, Self::Error> {
        self.recv.poll_trailers().map_err(Into::into)
    }

    fn is_end_stream(&self) -> bool {
        self.recv.is_end_stream()
    }
}

// ===== impl Data =====

impl Buf for Data {
    fn remaining(&self) -> usize {
        self.0.len()
    }

    fn bytes(&self) -> &[u8] {
        self.0.as_ref()
    }

    fn advance(&mut self, cnt: usize) {
        self.0.advance(cnt);
    }
}

// ===== impl Background =====

impl<S> Future for Background<S>
where
    S: HttpService<RequestBody>,
    S::Error: Into<BoxedStdError>,
    <S::ResponseBody as HttpBody>::Error: Into<BoxedStdError>,
{
    type Item = ();
    type Error = BoxedStdError;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        loop {
            self.state = match self.state {
                BackgroundState::Responding(ref mut respond) => {
                    match try_ready!(respond.poll_send_body()) {
                        Some(send_body) => BackgroundState::Sending(send_body),
                        None => return Ok(Async::Ready(())),
                    }
                }
                BackgroundState::Sending(ref mut send) => return send.poll_send(),
            };
        }
    }
}

// ===== impl Respond =====

impl<S> Respond<S>
where
    S: HttpService<RequestBody>,
    S::Error: Into<BoxedStdError>,
    <S::ResponseBody as HttpBody>::Error: Into<BoxedStdError>,
{
    fn poll_send_body(&mut self) -> Poll<Option<SendBody<S::ResponseBody>>, BoxedStdError> {
        let response = match self.future.poll() {
            Ok(Async::Ready(res)) => res,
            Ok(Async::NotReady) => {
                if let Async::Ready(reason) = self.reply.poll_reset()? {
                    log::debug!(
                        "received RST_STREAM before the response is resolved: {:?}",
                        reason
                    );
                    return Err(h2::Error::from(reason).into());
                }
                return Ok(Async::NotReady);
            }
            Err(err) => {
                let err = err.into();
                log::debug!("service respond error: {}", err);
                self.reply.send_reset(h2::Reason::INTERNAL_ERROR);
                return Err(err);
            }
        };

        let (parts, body) = response.into_parts();
        let mut response = Response::from_parts(parts, ());

        match response
            .headers_mut()
            .entry(http::header::DATE)
            .expect("DATE is a valid header name")
        {
            http::header::Entry::Occupied(..) => (),
            http::header::Entry::Vacant(entry) => {
                let date = HttpDate::from(SystemTime::now());
                let mut val = BytesMut::new();
                {
                    use std::io::Write;
                    let mut writer = (&mut val).writer();
                    let _ = write!(&mut writer, "{}", date);
                }
                let val = http::header::HeaderValue::from_shared(val.freeze())
                    .expect("formatted HttpDate must be a valid header value.");
                entry.insert(val);
            }
        }

        match response
            .headers_mut()
            .entry(http::header::CONTENT_LENGTH)
            .expect("CONTENT_LENGTH is a valid header name")
        {
            http::header::Entry::Occupied(..) => (),
            http::header::Entry::Vacant(entry) => {
                if let Some(len) = body.content_length() {
                    let mut val = BytesMut::new();
                    {
                        use std::io::Write;
                        let mut writer = (&mut val).writer();
                        let _ = write!(&mut writer, "{}", len);
                    }
                    let val = http::header::HeaderValue::from_shared(val.freeze())
                        .expect("formatted u64 must be a valid header value.");
                    entry.insert(val);
                }
            }
        }

        let end_of_stream = body.is_end_stream();
        let tx_stream = match self.reply.send_response(response, end_of_stream) {
            Ok(tx) => tx,
            Err(e) => {
                log::debug!("failed to send response: {}", e);
                self.reply.send_reset(h2::Reason::INTERNAL_ERROR);
                return Err(e.into());
            }
        };

        if end_of_stream {
            return Ok(Async::Ready(None));
        }

        Ok(Async::Ready(Some(SendBody {
            body,
            tx_stream,
            end_of_data: false,
        })))
    }
}

// ===== impl FlushBody ====

impl<B: HttpBody> SendBody<B>
where
    B::Error: Into<BoxedStdError>,
{
    fn send_data(&mut self, data: B::Data, end_of_stream: bool) -> Result<(), BoxedStdError> {
        self.tx_stream
            .send_data(SendBuf::Data(data), end_of_stream)
            .map_err(Into::into)
    }

    fn send_eos(&mut self, end_of_stream: bool) -> Result<(), BoxedStdError> {
        self.tx_stream
            .send_data(SendBuf::Eos, end_of_stream)
            .map_err(Into::into)
    }

    fn on_service_error(&mut self, err: B::Error) -> BoxedStdError {
        let err = err.into();
        log::debug!("body error: {}", err);
        self.tx_stream.send_reset(h2::Reason::INTERNAL_ERROR);
        err
    }

    fn poll_send(&mut self) -> Poll<(), BoxedStdError> {
        loop {
            if let Async::Ready(reason) = self.tx_stream.poll_reset()? {
                log::debug!("received RST_STREAM before sending a frame: {:?}", reason);
                return Err(h2::Error::from(reason).into());
            }

            if !self.end_of_data {
                match try_ready!(self.body.poll_data().map_err(|e| self.on_service_error(e))) {
                    Some(data) => {
                        let end_of_stream = self.body.is_end_stream();
                        self.send_data(data, end_of_stream)?;
                        if end_of_stream {
                            return Ok(Async::Ready(()));
                        }
                        continue;
                    }
                    None => {
                        self.end_of_data = true;
                        let end_of_stream = self.body.is_end_stream();
                        self.send_eos(end_of_stream)?;
                        if end_of_stream {
                            return Ok(Async::Ready(()));
                        }
                    }
                }
            } else {
                match try_ready!(self
                    .body
                    .poll_trailers()
                    .map_err(|e| self.on_service_error(e)))
                {
                    Some(trailers) => self.tx_stream.send_trailers(trailers)?,
                    None => self.send_eos(true)?,
                }
                return Ok(Async::Ready(()));
            }
        }
    }
}

// ===== impl SendBuf =====

impl<T: Buf> Buf for SendBuf<T> {
    fn remaining(&self) -> usize {
        match self {
            SendBuf::Data(ref data) => data.remaining(),
            SendBuf::Eos => 0,
        }
    }

    fn bytes(&self) -> &[u8] {
        match self {
            SendBuf::Data(ref data) => data.bytes(),
            SendBuf::Eos => &[],
        }
    }

    fn advance(&mut self, cnt: usize) {
        if let SendBuf::Data(ref mut data) = self {
            data.advance(cnt);
        }
    }
}
