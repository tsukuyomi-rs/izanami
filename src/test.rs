//! Utilities for testing HTTP services.

mod input;
mod output;

pub use self::{
    input::{Input, IntoRequestBody},
    output::Output,
};

use {
    crate::{CritError, RequestBody},
    bytes::{Buf, Bytes},
    cookie::Cookie,
    futures::{Async, Future, Poll},
    http::{
        header::{COOKIE, SET_COOKIE},
        Request, Response,
    },
    izanami_http::{BufStream, IntoBufStream},
    izanami_service::{MakeService, Service},
    std::{collections::HashMap, mem},
};

/// A set of extension methods of [`Response`] used within test cases.
///
/// [`Response`]: https://docs.rs/http/0.1/http/struct.Response.html
pub trait ResponseExt {
    /// Gets a reference to the header field with the specified name.
    ///
    /// If the header field does not exist, this method will return an `Err` instead of `None`.
    fn header<H>(&self, name: H) -> crate::Result<&http::header::HeaderValue>
    where
        H: http::header::AsHeaderName + std::fmt::Display;
}

impl<T> ResponseExt for http::Response<T> {
    fn header<H>(&self, name: H) -> crate::Result<&http::header::HeaderValue>
    where
        H: http::header::AsHeaderName + std::fmt::Display,
    {
        let err = failure::format_err!("missing header field: `{}'", name);
        self.headers()
            .get(name)
            .ok_or_else(|| crate::Error::from(err))
    }
}

/// Creates a test server using the specified service factory.
pub fn server<S, Bd>(make_service: S) -> crate::Result<Server<S, tokio::runtime::Runtime>>
where
    S: MakeService<(), Request<RequestBody>, Response = Response<Bd>>,
    S::Error: Into<crate::CritError>,
    S::Service: Send + 'static,
    <S::Service as Service<Request<RequestBody>>>::Future: Send + 'static,
    S::MakeError: Into<crate::CritError>,
    S::Future: Send + 'static,
    Bd: IntoBufStream,
    Bd::Stream: Send + 'static,
    Bd::Error: Into<CritError>,
{
    let mut builder = tokio::runtime::Builder::new();
    builder.core_threads(1);
    builder.blocking_threads(1);
    builder.name_prefix("izanami");
    let runtime = builder.build()?;
    Ok(Server::new(make_service, runtime))
}

/// Creates a test server that exexutes all task onto a single thread,
/// using the specified service factory.
pub fn local_server<S, Bd>(
    make_service: S,
) -> crate::Result<Server<S, tokio::runtime::current_thread::Runtime>>
where
    S: MakeService<(), Request<RequestBody>, Response = Response<Bd>>,
    S::Error: Into<crate::CritError>,
    S::MakeError: Into<crate::CritError>,
    Bd: IntoBufStream,
    Bd::Error: Into<CritError>,
{
    let runtime = tokio::runtime::current_thread::Runtime::new()?;
    Ok(Server::new(make_service, runtime))
}

/// A test server which emulates an HTTP service without using the low-level I/O.
#[derive(Debug)]
pub struct Server<S, Rt = tokio::runtime::Runtime> {
    make_service: S,
    runtime: Rt,
}

impl<S, Rt> Server<S, Rt>
where
    S: MakeService<(), Request<RequestBody>>,
{
    /// Creates an instance of `TestServer` from the specified components.
    pub fn new(make_service: S, runtime: Rt) -> Self {
        Self {
            make_service,
            runtime,
        }
    }
}

/// A type which manages a series of requests.
#[derive(Debug)]
pub struct Session<'a, S, Rt> {
    service: S,
    cookies: Option<HashMap<String, String>>,
    runtime: &'a mut Rt,
}

impl<'a, S, Rt> Session<'a, S, Rt>
where
    S: Service<Request<RequestBody>>,
{
    fn new(service: S, runtime: &'a mut Rt) -> Self {
        Session {
            service,
            runtime,
            cookies: None,
        }
    }

    /// Sets whether to save the Cookie entries or not.
    ///
    /// The default value is `false`.
    pub fn save_cookies(mut self, enabled: bool) -> Self {
        if enabled {
            self.cookies.get_or_insert_with(Default::default);
        } else {
            self.cookies.take();
        }
        self
    }

    pub fn cookie(&self, name: &str) -> Option<&str> {
        self.cookies.as_ref()?.get(name).map(|s| s.as_str())
    }

    /// Returns the reference to the underlying Tokio runtime.
    pub fn runtime(&mut self) -> &mut Rt {
        &mut *self.runtime
    }

    fn build_request<T>(&self, input: T) -> crate::Result<Request<hyper::Body>>
    where
        T: Input,
    {
        let mut request = input.build_request()?;
        if let Some(cookies) = &self.cookies {
            for (k, v) in cookies {
                request.headers_mut().append(
                    COOKIE,
                    Cookie::new(k.to_owned(), v.to_owned())
                        .to_string()
                        .parse()?,
                );
            }
        }
        Ok(request)
    }

    fn handle_set_cookies(&mut self, response: &Response<Output>) -> crate::Result<()> {
        if let Some(ref mut cookies) = &mut self.cookies {
            for set_cookie in response.headers().get_all(SET_COOKIE) {
                let cookie = Cookie::parse_encoded(set_cookie.to_str()?)?;
                if cookie.value().is_empty() {
                    cookies.remove(cookie.name());
                } else {
                    cookies.insert(cookie.name().to_owned(), cookie.value().to_owned());
                }
            }
        }
        Ok(())
    }
}

mod threadpool {
    use {
        super::*,
        std::panic::{resume_unwind, AssertUnwindSafe},
        tokio::runtime::Runtime,
    };

    fn block_on<F>(runtime: &mut Runtime, future: F) -> Result<F::Item, F::Error>
    where
        F: Future + Send + 'static,
        F::Item: Send + 'static,
        F::Error: Send + 'static,
    {
        match runtime.block_on(AssertUnwindSafe(future).catch_unwind()) {
            Ok(result) => result,
            Err(err) => resume_unwind(Box::new(err)),
        }
    }

    impl<S, Bd> Server<S, Runtime>
    where
        S: MakeService<(), Request<RequestBody>, Response = Response<Bd>>,
        S::Error: Into<CritError>,
        S::Future: Send + 'static,
        S::MakeError: Into<CritError> + Send + 'static,
        S::Service: Send + 'static,
        Bd: IntoBufStream,
        Bd::Stream: Send + 'static,
        Bd::Error: Into<CritError>,
    {
        /// Create a `Session` associated with this server.
        pub fn new_session(&mut self) -> crate::Result<Session<'_, S::Service, Runtime>> {
            let service = block_on(
                &mut self.runtime,
                self.make_service.make_service(()).map_err(Into::into),
            )
            .map_err(failure::Error::from_boxed_compat)?;

            Ok(Session::new(service, &mut self.runtime))
        }

        pub fn perform<T>(&mut self, input: T) -> crate::Result<Response<Output>>
        where
            T: Input,
            <S::Service as Service<Request<RequestBody>>>::Future: Send + 'static,
        {
            let mut session = self.new_session()?;
            session.perform(input)
        }
    }

    impl<'a, S, Bd> Session<'a, S, Runtime>
    where
        S: Service<Request<RequestBody>, Response = Response<Bd>>,
        S::Error: Into<CritError>,
        S::Future: Send + 'static,
        Bd: IntoBufStream,
        Bd::Stream: Send + 'static,
        Bd::Error: Into<CritError>,
    {
        /// Applies an HTTP request to this client and await its response.
        pub fn perform<T>(&mut self, input: T) -> crate::Result<Response<Output>>
        where
            T: Input,
        {
            let request = self.build_request(input)?;

            let future = TestResponseFuture::Initial(self.service.call(request.map(RequestBody)));
            let response =
                block_on(&mut self.runtime, future).map_err(failure::Error::from_boxed_compat)?;
            self.handle_set_cookies(&response)?;

            Ok(response)
        }
    }
}

mod current_thread {
    use {super::*, tokio::runtime::current_thread::Runtime};

    impl<S, Bd> Server<S, Runtime>
    where
        S: MakeService<(), Request<RequestBody>, Response = Response<Bd>>,
        S::Error: Into<CritError>,
        S::MakeError: Into<CritError>,
        Bd: IntoBufStream,
        Bd::Error: Into<CritError>,
    {
        /// Create a `Session` associated with this server.
        pub fn new_session(&mut self) -> crate::Result<Session<'_, S::Service, Runtime>> {
            let service = self
                .runtime
                .block_on(self.make_service.make_service(()))
                .map_err(|err| failure::Error::from_boxed_compat(err.into()))?;
            Ok(Session::new(service, &mut self.runtime))
        }

        pub fn perform<T>(&mut self, input: T) -> crate::Result<Response<Output>>
        where
            T: Input,
        {
            let mut session = self.new_session()?;
            session.perform(input)
        }
    }

    impl<'a, S, Bd> Session<'a, S, Runtime>
    where
        S: Service<Request<RequestBody>, Response = Response<Bd>>,
        S::Error: Into<CritError>,
        Bd: IntoBufStream,
        Bd::Error: Into<CritError>,
    {
        /// Applies an HTTP request to this client and await its response.
        pub fn perform<T>(&mut self, input: T) -> crate::Result<Response<Output>>
        where
            T: Input,
        {
            let request = self.build_request(input)?;

            let future = TestResponseFuture::Initial(self.service.call(request.map(RequestBody)));
            let response = self
                .runtime
                .block_on(future)
                .map_err(failure::Error::from_boxed_compat)?;
            self.handle_set_cookies(&response)?;

            Ok(response)
        }
    }
}

#[allow(missing_debug_implementations, clippy::large_enum_variant)]
enum TestResponseFuture<F, Bd> {
    Initial(F),
    Receive(http::response::Parts, Receive<Bd>),
    Done,
}

impl<F, Bd, T> Future for TestResponseFuture<F, Bd>
where
    F: Future<Item = Response<T>>,
    F::Error: Into<CritError>,
    Bd: BufStream,
    Bd::Error: Into<CritError>,
    T: IntoBufStream<Item = Bd::Item, Error = Bd::Error, Stream = Bd>,
{
    type Item = Response<Output>;
    type Error = CritError;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        use self::TestResponseFuture::*;
        loop {
            let response = match *self {
                Initial(ref mut f) => {
                    let response = futures::try_ready!(f.poll().map_err(Into::into));
                    Some(response)
                }
                Receive(_, ref mut receive) => {
                    futures::try_ready!(receive.poll_ready().map_err(Into::into));
                    None
                }
                _ => unreachable!("unexpected state"),
            };

            match mem::replace(self, TestResponseFuture::Done) {
                TestResponseFuture::Initial(..) => {
                    let response = response.expect("unexpected condition");
                    let (parts, body) = response.into_parts();
                    let receive = self::Receive::new(body.into_buf_stream());
                    *self = TestResponseFuture::Receive(parts, receive);
                }
                TestResponseFuture::Receive(parts, receive) => {
                    let data = receive.into_data().expect("unexpected condition");
                    let response = Response::from_parts(parts, data);
                    return Ok(response.into());
                }
                _ => unreachable!("unexpected state"),
            }
        }
    }
}

#[allow(missing_debug_implementations)]
struct Receive<Bd> {
    state: ReceiveState<Bd>,
}

#[allow(missing_debug_implementations)]
enum ReceiveState<Bd> {
    Init(Option<Bd>),
    InFlight { body: Bd, chunks: Vec<Bytes> },
    Ready(Output),
}

impl<Bd> Receive<Bd>
where
    Bd: BufStream,
{
    fn new(body: Bd) -> Self {
        Self {
            state: ReceiveState::Init(Some(body)),
        }
    }

    fn poll_ready(&mut self) -> Poll<(), Bd::Error> {
        loop {
            self.state = match self.state {
                ReceiveState::Init(ref mut body) => ReceiveState::InFlight {
                    body: body.take().expect("unexpected condition"),
                    chunks: vec![],
                },
                ReceiveState::InFlight {
                    ref mut body,
                    ref mut chunks,
                } => {
                    while let Some(chunk) = futures::try_ready!(body.poll_buf()) {
                        chunks.push(chunk.collect());
                    }
                    self.state = ReceiveState::Ready(Output {
                        chunks: mem::replace(chunks, vec![]),
                    });
                    return Ok(Async::Ready(()));
                }
                ReceiveState::Ready(..) => return Ok(Async::Ready(())),
            }
        }
    }

    pub(super) fn into_data(self) -> Option<Output> {
        match self.state {
            ReceiveState::Ready(data) => Some(data),
            _ => None,
        }
    }
}
