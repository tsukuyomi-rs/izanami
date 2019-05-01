use {
    crate::error::Error,
    crate::{
        body::{Body, HttpBody},
        context::ContextData,
        handler::Handler,
        server::{service::Service, upgrade::HttpUpgrade, Connection},
        util::MapAsyncOptExt,
        ws::WebSocketDriver,
        VERSION_STR,
    },
    either::Either,
    futures::{Async, Future, Poll},
    http::{
        header::{HeaderValue, SERVER, SET_COOKIE},
        Request, Response,
    },
    std::{convert::Infallible, io},
    std::{rc::Rc, sync::Arc},
    tokio::io::{AsyncRead, AsyncWrite},
    tokio_tungstenite::WebSocketStream,
    tungstenite::protocol::Role,
};

/// The main trait that abstracts Web applications.
pub trait App {
    type Body: HttpBody;
    type Error: Into<Error>;
    type Handler: Handler<Body = Self::Body, Error = Self::Error>;

    /// Creates an instance of `Handler` to handle an incoming request.
    fn new_handler(&self) -> Self::Handler;
}

impl<F, H> App for F
where
    F: Fn() -> H,
    H: Handler,
{
    type Body = H::Body;
    type Error = H::Error;
    type Handler = H;

    fn new_handler(&self) -> Self::Handler {
        (*self)()
    }
}

impl<T> App for Rc<T>
where
    T: App + ?Sized,
{
    type Body = T::Body;
    type Error = T::Error;
    type Handler = T::Handler;

    fn new_handler(&self) -> Self::Handler {
        (**self).new_handler()
    }
}

impl<T> App for Arc<T>
where
    T: App + ?Sized,
{
    type Body = T::Body;
    type Error = T::Error;
    type Handler = T::Handler;

    fn new_handler(&self) -> Self::Handler {
        (**self).new_handler()
    }
}

#[allow(missing_debug_implementations)]
pub(crate) struct AppService<T: App> {
    app: T,
}

impl<T: App> From<T> for AppService<T> {
    fn from(app: T) -> Self {
        Self { app }
    }
}

impl<T, B> Service<Request<B>> for AppService<T>
where
    T: App,
    Body: From<B>,
{
    type Response = Response<AppBody<T::Body>>;
    type Error = Infallible;
    type Future = AppServiceFuture<T::Handler>;

    fn poll_ready(&mut self) -> Poll<(), Self::Error> {
        Ok(Async::Ready(()))
    }

    fn call(&mut self, request: Request<B>) -> Self::Future {
        AppServiceFuture {
            handler: self.app.new_handler(),
            data: ContextData::new(request.map(Body::from)),
        }
    }
}

#[allow(missing_debug_implementations)]
pub(crate) struct AppServiceFuture<H> {
    handler: H,
    data: ContextData,
}

impl<H> Future for AppServiceFuture<H>
where
    H: Handler,
{
    type Item = Response<AppBody<H::Body>>;
    type Error = Infallible;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let mut response = match self.handler.poll_http(&mut self.data.context()) {
            Ok(Async::Ready(res)) => res.map(AppBodyKind::Success),
            Ok(Async::NotReady) => return Ok(Async::NotReady),
            Err(err) => {
                let err = err.into();
                let msg = err.to_string();
                let mut response = err.to_response().map(|_| AppBodyKind::Errored(Some(msg)));
                response.extensions_mut().insert(err);
                response
            }
        };

        if let Some(hdrs) = self.data.response_headers.take() {
            response.headers_mut().extend(hdrs);
        }

        if let Some(ref jar) = self.data.cookies {
            for cookie in jar.delta() {
                let val = cookie
                    .encoded()
                    .to_string()
                    .parse()
                    .expect("Cookie<'_> must be a valid header value");
                response.headers_mut().append(SET_COOKIE, val);
            }
        }

        response
            .headers_mut()
            .entry(SERVER)
            .expect("valid header name")
            .or_insert_with(|| HeaderValue::from_static(VERSION_STR));

        Ok(Async::Ready(response.map(|kind| AppBody {
            kind,
            ws_driver: self.data.ws_driver.take(),
        })))
    }
}

#[allow(missing_debug_implementations)]
pub(crate) struct AppBody<B> {
    kind: AppBodyKind<B>,
    ws_driver: Option<WebSocketDriver>,
}

enum AppBodyKind<B> {
    Success(B),
    Errored(Option<String>),
}

impl<B> HttpBody for AppBody<B>
where
    B: HttpBody,
{
    type Data = Either<B::Data, io::Cursor<String>>;
    type Error = B::Error;

    fn poll_data(&mut self) -> Poll<Option<Self::Data>, <Self as HttpBody>::Error> {
        match self.kind {
            AppBodyKind::Success(ref mut body) => body.poll_data().map_async_opt(Either::Left),
            AppBodyKind::Errored(ref mut msg) => Ok(Async::Ready(
                msg.take().map(|msg| Either::Right(io::Cursor::new(msg))),
            )),
        }
    }

    fn poll_trailers(&mut self) -> Poll<Option<http::HeaderMap>, <Self as HttpBody>::Error> {
        match self.kind {
            AppBodyKind::Success(ref mut body) => body.poll_trailers(),
            _ => Ok(Async::Ready(None)),
        }
    }

    fn size_hint(&self) -> tokio_buf::SizeHint {
        match self.kind {
            AppBodyKind::Success(ref body) => body.size_hint(),
            AppBodyKind::Errored(ref msg) => {
                let mut hint = tokio_buf::SizeHint::new();
                if let Some(ref msg) = msg {
                    let len = msg.len() as u64;
                    hint.set_upper(len);
                    hint.set_lower(len);
                }
                hint
            }
        }
    }

    fn is_end_stream(&self) -> bool {
        match self.kind {
            AppBodyKind::Success(ref body) => body.is_end_stream(),
            AppBodyKind::Errored(ref msg) => msg.is_none(),
        }
    }

    fn content_length(&self) -> Option<u64> {
        match self.kind {
            AppBodyKind::Success(ref body) => body.content_length(),
            AppBodyKind::Errored(ref msg) => msg.as_ref().map(|msg| msg.len() as u64),
        }
    }
}

impl<B, I> HttpUpgrade<I> for AppBody<B>
where
    B: HttpBody,
    I: AsyncRead + AsyncWrite + 'static,
{
    type Upgraded = AppConnection<I>;
    type UpgradeError = crate::ws::Error;

    fn upgrade(self, io: I) -> Result<Self::Upgraded, I> {
        match (self.kind, self.ws_driver) {
            (AppBodyKind::Success(_body), Some(driver)) => {
                let socket = WebSocketStream::from_raw_socket(io, Role::Server, None);
                Ok(AppConnection {
                    socket,
                    driver: Fuse(Some(driver)),
                })
            }
            _ => Err(io),
        }
    }
}

#[allow(dead_code)]
pub(crate) struct AppConnection<I> {
    socket: WebSocketStream<I>,
    driver: Fuse<WebSocketDriver>,
}

impl<I> Connection for AppConnection<I>
where
    I: AsyncRead + AsyncWrite + 'static,
{
    type Error = crate::ws::Error;

    fn poll_close(&mut self) -> Poll<(), Self::Error> {
        let socket = &mut self.socket;
        self.driver.poll_fuse(|backend| backend.poll(socket))
    }

    fn graceful_shutdown(&mut self) {}
}

#[derive(Debug)]
struct Fuse<Fut>(Option<Fut>);

impl<Fut> Fuse<Fut> {
    fn poll_fuse<F, E>(&mut self, f: F) -> Poll<(), E>
    where
        F: FnOnce(&mut Fut) -> Poll<(), E>,
    {
        let res = self.0.as_mut().map(f);
        match res.unwrap_or_else(|| Ok(Async::Ready(()))) {
            res @ Ok(Async::Ready(..)) | res @ Err(..) => {
                self.0 = None;
                res
            }
            Ok(Async::NotReady) => Ok(Async::NotReady),
        }
    }
}
