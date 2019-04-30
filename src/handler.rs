use {
    crate::{
        body::Body,
        context::{Context, ContextInner},
        error::Error,
    },
    either::Either,
    futures::{Async, Future, Poll},
    http::{
        header::{HeaderValue, SERVER, SET_COOKIE},
        Request, Response,
    },
    izanami_http::{
        body::HttpBody,
        upgrade::{HttpUpgrade, Upgraded},
        ResponseBody,
    },
    izanami_service::Service,
    izanami_util::MapAsyncOptExt,
    std::{convert::Infallible, io, rc::Rc, sync::Arc},
};

const VERSION_STR: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"));

/// Asynchronous HTTP handler dispatched per incoming requests.
pub trait Handler {
    type Body: ResponseBody;
    type Error: Into<Error>;

    fn poll_response(&mut self, cx: &mut Context<'_>) -> Poll<Response<Self::Body>, Self::Error>;
}

impl<F, B> Handler for F
where
    F: Future<Item = Response<B>>,
    F::Error: Into<Error>,
    B: ResponseBody,
{
    type Body = B;
    type Error = F::Error;

    fn poll_response(&mut self, cx: &mut Context<'_>) -> Poll<Response<Self::Body>, Self::Error> {
        crate::context::set_tls_context(cx, || Future::poll(self))
    }
}

/// A factory of `Handlers`.
pub trait NewHandler {
    type Body: ResponseBody;
    type Error: Into<Error>;
    type Handler: Handler<Body = Self::Body, Error = Self::Error>;

    fn new_handler(&self) -> Self::Handler;
}

impl<F, H> NewHandler for F
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

impl<H> NewHandler for Rc<H>
where
    H: NewHandler + ?Sized,
{
    type Body = H::Body;
    type Error = H::Error;
    type Handler = H::Handler;

    fn new_handler(&self) -> Self::Handler {
        (**self).new_handler()
    }
}

impl<H> NewHandler for Arc<H>
where
    H: NewHandler + ?Sized,
{
    type Body = H::Body;
    type Error = H::Error;
    type Handler = H::Handler;

    fn new_handler(&self) -> Self::Handler {
        (**self).new_handler()
    }
}

pub trait ModifyHandler<H: Handler> {
    type Body: ResponseBody;
    type Error: Into<Error>;
    type Handler: Handler<Body = Self::Body, Error = Self::Error>;

    fn modify_handler(&self, handler: H) -> Self::Handler;
}

impl<M, H> ModifyHandler<H> for Rc<M>
where
    M: ModifyHandler<H> + ?Sized,
    H: Handler,
{
    type Body = M::Body;
    type Error = M::Error;
    type Handler = M::Handler;

    fn modify_handler(&self, handler: H) -> Self::Handler {
        (**self).modify_handler(handler)
    }
}

impl<M, H> ModifyHandler<H> for Arc<M>
where
    M: ModifyHandler<H> + ?Sized,
    H: Handler,
{
    type Body = M::Body;
    type Error = M::Error;
    type Handler = M::Handler;

    fn modify_handler(&self, handler: H) -> Self::Handler {
        (**self).modify_handler(handler)
    }
}

pub(crate) struct HandlerService<H>(pub(crate) H);

impl<H, B> Service<Request<B>> for HandlerService<H>
where
    H: NewHandler,
    B: HttpBody + Send + 'static,
    B::Data: Send + 'static,
    B::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
{
    type Response = Response<HandlerServiceBody<H::Body>>;
    type Error = Infallible;
    type Future = HandlerServiceFuture<H::Handler>;

    fn poll_ready(&mut self) -> Poll<(), Self::Error> {
        Ok(Async::Ready(()))
    }

    fn call(&mut self, request: Request<B>) -> Self::Future {
        HandlerServiceFuture {
            handler: self.0.new_handler(),
            context: ContextInner::new(request.map(Body::new)),
        }
    }
}

pub(crate) struct HandlerServiceFuture<H> {
    handler: H,
    context: ContextInner,
}

impl<H> Future for HandlerServiceFuture<H>
where
    H: Handler,
{
    type Item = Response<HandlerServiceBody<H::Body>>;
    type Error = Infallible;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let mut response = match self.handler.poll_response(&mut Context(&mut self.context)) {
            Ok(Async::Ready(res)) => res.map(HandlerServiceBody::Success),
            Ok(Async::NotReady) => return Ok(Async::NotReady),
            Err(err) => {
                let err = err.into();
                let msg = err.to_string();
                let mut response = err
                    .to_response()
                    .map(|_| HandlerServiceBody::Errored(Some(msg)));
                response.extensions_mut().insert(err);
                response
            }
        };

        if let Some(hdrs) = self.context.response_headers.take() {
            response.headers_mut().extend(hdrs);
        }

        if let Some(ref jar) = self.context.cookies {
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

        Ok(Async::Ready(response))
    }
}

pub(crate) enum HandlerServiceBody<B> {
    Success(B),
    Errored(Option<String>),
}

impl<B> HttpBody for HandlerServiceBody<B>
where
    B: ResponseBody,
{
    type Data = Either<B::Data, io::Cursor<String>>;
    type Error = B::Error;

    fn poll_data(&mut self) -> Poll<Option<Self::Data>, <Self as HttpBody>::Error> {
        match self {
            HandlerServiceBody::Success(ref mut body) => {
                body.poll_data().map_async_opt(Either::Left)
            }
            HandlerServiceBody::Errored(ref mut msg) => Ok(Async::Ready(
                msg.take().map(|msg| Either::Right(io::Cursor::new(msg))),
            )),
        }
    }

    fn poll_trailers(&mut self) -> Poll<Option<http::HeaderMap>, <Self as HttpBody>::Error> {
        match self {
            HandlerServiceBody::Success(ref mut body) => body.poll_trailers(),
            _ => Ok(Async::Ready(None)),
        }
    }

    fn size_hint(&self) -> tokio_buf::SizeHint {
        match self {
            HandlerServiceBody::Success(ref body) => body.size_hint(),
            HandlerServiceBody::Errored(ref msg) => {
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
        match self {
            HandlerServiceBody::Success(ref body) => body.is_end_stream(),
            HandlerServiceBody::Errored(ref msg) => msg.is_none(),
        }
    }

    fn content_length(&self) -> Option<u64> {
        match self {
            HandlerServiceBody::Success(ref body) => body.content_length(),
            HandlerServiceBody::Errored(ref msg) => msg.as_ref().map(|msg| msg.len() as u64),
        }
    }
}

impl<B> HttpUpgrade for HandlerServiceBody<B>
where
    B: HttpUpgrade,
{
    type UpgradeError = B::UpgradeError;

    fn poll_upgraded(&mut self, io: &mut Upgraded<'_>) -> Poll<(), Self::UpgradeError> {
        match self {
            HandlerServiceBody::Success(body) => body.poll_upgraded(io),
            _ => Ok(Async::Ready(())),
        }
    }

    fn notify_shutdown(&mut self) {
        if let HandlerServiceBody::Success(body) = self {
            body.notify_shutdown()
        }
    }
}
