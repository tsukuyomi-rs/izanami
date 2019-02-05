use {
    crate::{error::BoxedStdError, io::Listener},
    bytes::Buf,
    futures::{Future, Poll},
    http::{HeaderMap, Request, Response},
    hyper::body::Payload as _Payload,
    izanami_service::{MakeService, Service},
    izanami_util::{
        buf_stream::{BufStream, SizeHint},
        http::{HasTrailers, Upgrade},
    },
    std::marker::PhantomData,
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

// ==== Context ====

/// A type representing the context information that can be used from the inside
/// of `MakeService::make_service`.
#[derive(Debug)]
pub struct Context<'a, T: Listener> {
    conn: &'a T::Conn,
    _anchor: PhantomData<std::rc::Rc<()>>,
}

impl<'a, T> Context<'a, T>
where
    T: Listener,
{
    pub(crate) fn new(conn: &'a T::Conn) -> Self {
        Self {
            conn,
            _anchor: PhantomData,
        }
    }

    /// Returns a reference to the instance of a connection to a peer.
    pub fn conn(&self) -> &T::Conn {
        &*self.conn
    }
}

impl<'a, T> std::ops::Deref for Context<'a, T>
where
    T: Listener,
{
    type Target = T::Conn;

    fn deref(&self) -> &Self::Target {
        self.conn()
    }
}

pub trait MakeHttpService<T: Listener> {
    type ResponseBody: ResponseBody + Send + 'static;
    type Error: Into<BoxedStdError>;
    type Service: HttpService<ResponseBody = Self::ResponseBody, Error = Self::Error>;
    type MakeError: Into<BoxedStdError>;
    type Future: Future<Item = Self::Service, Error = Self::MakeError>;

    fn make_service(&mut self, cx: Context<'_, T>) -> Self::Future;
}

impl<S, T, Bd, SvcErr, MkErr, Svc, Fut> MakeHttpService<T> for S
where
    T: Listener,
    S: for<'a> MakeService<
        Context<'a, T>, //
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

    fn make_service(&mut self, cx: Context<'_, T>) -> Self::Future {
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

    fn poll_buf(&mut self) -> Poll<Option<Self::Item>, Self::Error>;

    fn size_hint(&self) -> SizeHint;
}

impl<Bd> ResponseBody for Bd
where
    Bd: BufStream,
    Bd::Item: Send,
    Bd::Error: Into<BoxedStdError>,
{
    type Item = Bd::Item;
    type Error = Bd::Error;

    fn poll_buf(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        BufStream::poll_buf(self)
    }

    fn size_hint(&self) -> SizeHint {
        BufStream::size_hint(self)
    }
}
