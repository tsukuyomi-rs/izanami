use {
    futures::{Async, Poll},
    http::{Request, Response, StatusCode},
    izanami_buf_stream::BufStream,
    izanami_service::{MakeService, Service},
    regex::{Captures, Regex, RegexSet},
    std::sync::Arc,
};

pub struct ResponseBody(Option<String>);

impl<T: Into<String>> From<T> for ResponseBody {
    fn from(data: T) -> Self {
        ResponseBody(Some(data.into()))
    }
}

impl BufStream for ResponseBody {
    type Item = std::io::Cursor<String>;
    type Error = std::io::Error;

    fn poll_buf(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        Ok(Async::Ready(self.0.take().map(std::io::Cursor::new)))
    }

    fn is_end_stream(&self) -> bool {
        self.0.is_none()
    }
}

pub struct Context<'a, Bd> {
    request: Request<Bd>,
    regex: &'a Regex,
}

impl<'a, Bd> Context<'a, Bd> {
    pub fn into_request(self) -> Request<Bd> {
        self.request
    }

    pub fn captures(&self) -> Option<Captures<'_>> {
        self.regex.captures(self.request.uri().path())
    }
}

impl<'a, Bd> std::ops::Deref for Context<'a, Bd> {
    type Target = Request<Bd>;

    fn deref(&self) -> &Self::Target {
        &self.request
    }
}

impl<'a, Bd> std::ops::DerefMut for Context<'a, Bd> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.request
    }
}

type HandlerFn<Bd> = dyn Fn(Context<'_, Bd>) -> Response<ResponseBody> + Send + Sync + 'static;

struct Inner<Bd> {
    regex_set: RegexSet,
    routes: Vec<(Regex, Box<HandlerFn<Bd>>)>,
}

pub struct Builder<Bd> {
    routes: Vec<(Regex, Box<HandlerFn<Bd>>)>,
}

impl<Bd> Default for Builder<Bd> {
    fn default() -> Self {
        Self { routes: vec![] }
    }
}

impl<Bd> Builder<Bd> {
    pub fn add_route<H, T>(mut self, pattern: &str, handler: H) -> Result<Self, regex::Error>
    where
        H: Fn(Context<'_, Bd>) -> Response<T> + Send + Sync + 'static,
        T: Into<ResponseBody>,
    {
        let pattern = Regex::new(pattern)?;
        self.routes
            .push((pattern, Box::new(move |cx| (handler)(cx).map(Into::into))));
        Ok(self)
    }

    pub fn build(self) -> Echo<Bd> {
        let regex_set = RegexSet::new(
            self.routes
                .iter() //
                .map(|route| route.0.as_str()),
        )
        .expect("Regex should be a valid regex pattern");

        Echo {
            inner: Arc::new(Inner {
                regex_set,
                routes: self.routes,
            }),
        }
    }
}

pub struct Echo<Bd = ()> {
    inner: Arc<Inner<Bd>>,
}

impl<Bd> Echo<Bd> {
    pub fn builder() -> Builder<Bd> {
        Builder::default()
    }
}

mod imp {
    use super::*;

    impl<Ctx, Bd> MakeService<Ctx, Request<Bd>> for Echo<Bd> {
        type Response = Response<ResponseBody>;
        type Error = std::io::Error;
        type Service = EchoService<Bd>;
        type MakeError = std::io::Error;
        type Future = futures::future::FutureResult<Self::Service, Self::MakeError>;

        fn make_service(&self, _: Ctx) -> Self::Future {
            futures::future::ok(EchoService {
                inner: self.inner.clone(),
            })
        }
    }

    pub struct EchoService<Bd> {
        inner: Arc<Inner<Bd>>,
    }

    impl<Bd> Service<Request<Bd>> for EchoService<Bd> {
        type Response = Response<ResponseBody>;
        type Error = std::io::Error;
        type Future = futures::future::FutureResult<Self::Response, Self::Error>;

        fn poll_ready(&mut self) -> Poll<(), Self::Error> {
            Ok(Async::Ready(()))
        }

        fn call(&mut self, request: Request<Bd>) -> Self::Future {
            if let Some((regex, handler)) = self
                .inner
                .regex_set
                .matches(request.uri().path())
                .iter()
                .next()
                .and_then(|i| self.inner.routes.get(i))
            {
                futures::future::ok((*handler)(Context { request, regex }))
            } else {
                futures::future::ok(
                    Response::builder()
                        .status(StatusCode::NOT_FOUND)
                        .body("not found".into())
                        .expect("should be a valid response"),
                )
            }
        }
    }
}
