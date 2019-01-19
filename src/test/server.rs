use {
    super::{
        input::{Input, MockRequestBody},
        output::Output,
    },
    crate::{remote::RemoteAddr, CritError},
    cookie::Cookie,
    http::{
        header::{COOKIE, SET_COOKIE},
        Request, Response,
    },
    izanami_service::{MakeService, Service},
    std::collections::HashMap,
};

/// A trait that represents the runtime for executing asynchronous processes
/// within the specified `MakeService`.
pub trait Runtime<S>: self::imp::RuntimeImpl<S>
where
    S: MakeService<(), Request<MockRequestBody>>,
{
}

mod imp {
    use {
        super::*,
        bytes::{Buf, Bytes},
        futures::{Async, Future, Poll},
        izanami_util::buf_stream::BufStream,
        std::{
            mem,
            panic::{resume_unwind, AssertUnwindSafe},
        },
    };

    pub trait RuntimeImpl<S>
    where
        S: MakeService<(), Request<MockRequestBody>>,
    {
        fn make_service(&mut self, future: S::Future) -> crate::Result<S::Service>;

        fn call(
            &mut self,
            future: <S::Service as Service<Request<MockRequestBody>>>::Future,
        ) -> crate::Result<Response<Output>>;

        fn shutdown(self);
    }

    fn block_on<F>(runtime: &mut tokio::runtime::Runtime, future: F) -> Result<F::Item, F::Error>
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

    impl<S, Bd> Runtime<S> for tokio::runtime::Runtime
    where
        S: MakeService<(), Request<MockRequestBody>, Response = Response<Bd>>,
        S::Error: Into<crate::CritError>,
        S::Service: Send + 'static,
        <S::Service as Service<Request<MockRequestBody>>>::Future: Send + 'static,
        S::MakeError: Into<crate::CritError> + 'static,
        S::Future: Send + 'static,
        Bd: BufStream + Send + 'static,
        Bd::Error: Into<CritError>,
    {
    }

    impl<S, Bd> RuntimeImpl<S> for tokio::runtime::Runtime
    where
        S: MakeService<(), Request<MockRequestBody>, Response = Response<Bd>>,
        S::Error: Into<crate::CritError>,
        S::Service: Send + 'static,
        <S::Service as Service<Request<MockRequestBody>>>::Future: Send + 'static,
        S::MakeError: Into<crate::CritError> + 'static,
        S::Future: Send + 'static,
        Bd: BufStream + Send + 'static,
        Bd::Error: Into<CritError>,
    {
        fn make_service(&mut self, future: S::Future) -> crate::Result<S::Service> {
            block_on(self, future.map_err(Into::into))
                .map_err(failure::Error::from_boxed_compat)
                .map_err(Into::into)
        }

        fn call(
            &mut self,
            mut future: <S::Service as Service<Request<MockRequestBody>>>::Future,
        ) -> crate::Result<Response<Output>> {
            let (parts, body) = block_on(
                self,
                futures::future::poll_fn(move || {
                    future
                        .poll()
                        .map_err(Into::into)
                        .map_err(failure::Error::from_boxed_compat)
                }),
            )?
            .into_parts();

            let output = block_on(
                self,
                Receive {
                    state: ReceiveState::Init(Some(body)),
                },
            )?;

            Ok(Response::from_parts(parts, output))
        }

        fn shutdown(self) {
            self.shutdown_on_idle().wait().unwrap();
        }
    }

    impl<S, Bd> Runtime<S> for tokio::runtime::current_thread::Runtime
    where
        S: MakeService<(), Request<MockRequestBody>, Response = Response<Bd>>,
        S::Error: Into<crate::CritError>,
        S::MakeError: Into<crate::CritError>,
        Bd: BufStream,
        Bd::Error: Into<CritError>,
    {
    }

    impl<S, Bd> RuntimeImpl<S> for tokio::runtime::current_thread::Runtime
    where
        S: MakeService<(), Request<MockRequestBody>, Response = Response<Bd>>,
        S::Error: Into<crate::CritError>,
        S::MakeError: Into<crate::CritError>,
        Bd: BufStream,
        Bd::Error: Into<CritError>,
    {
        fn make_service(&mut self, future: S::Future) -> crate::Result<S::Service> {
            self.block_on(future)
                .map_err(|err| failure::Error::from_boxed_compat(err.into()))
                .map_err(Into::into)
        }

        fn call(
            &mut self,
            mut future: <S::Service as Service<Request<MockRequestBody>>>::Future,
        ) -> crate::Result<Response<Output>> {
            let (parts, body) = self
                .block_on(futures::future::poll_fn(move || {
                    future
                        .poll()
                        .map_err(Into::into)
                        .map_err(failure::Error::from_boxed_compat)
                }))?
                .into_parts();

            let output = self.block_on(Receive {
                state: ReceiveState::Init(Some(body)),
            })?;

            Ok(Response::from_parts(parts, output))
        }

        fn shutdown(mut self) {
            self.run().unwrap();
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
    }

    impl<Bd> Future for Receive<Bd>
    where
        Bd: BufStream,
        Bd::Error: Into<CritError>,
    {
        type Item = Output;
        type Error = crate::Error;

        fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
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
                        while let Some(chunk) = futures::try_ready!({
                            body.poll_buf()
                                .map_err(Into::into)
                                .map_err(failure::Error::from_boxed_compat)
                        }) {
                            chunks.push(chunk.collect());
                        }
                        return Ok(Async::Ready(Output {
                            chunks: mem::replace(chunks, vec![]),
                        }));
                    }
                }
            }
        }
    }
}

/// Creates a test server using the specified service factory.
pub fn server<S>(make_service: S) -> crate::Result<Server<S, tokio::runtime::Runtime>>
where
    S: MakeService<(), Request<MockRequestBody>>,
    tokio::runtime::Runtime: Runtime<S>,
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
pub fn local_server<S>(
    make_service: S,
) -> crate::Result<Server<S, tokio::runtime::current_thread::Runtime>>
where
    S: MakeService<(), Request<MockRequestBody>>,
    tokio::runtime::current_thread::Runtime: Runtime<S>,
{
    let runtime = tokio::runtime::current_thread::Runtime::new()?;
    Ok(Server::new(make_service, runtime))
}

/// A type that simulates an HTTP server without using the low-level I/O.
#[derive(Debug)]
pub struct Server<S, Rt = tokio::runtime::Runtime>
where
    S: MakeService<(), Request<MockRequestBody>>,
    Rt: Runtime<S>,
{
    make_service: S,
    cookies: Option<HashMap<String, String>>,
    remote_addr: RemoteAddr,
    runtime: Rt,
}

impl<S, Rt> Server<S, Rt>
where
    S: MakeService<(), Request<MockRequestBody>>,
    Rt: Runtime<S>,
{
    /// Creates a new `Server` using the specific service and runtime.
    pub fn new(make_service: S, runtime: Rt) -> Self {
        Self {
            make_service,
            cookies: None,
            remote_addr: RemoteAddr::tcp(([127, 0, 0, 1], 12345).into()),
            runtime,
        }
    }

    /// Sets whether to save the Cookie entries or not.
    ///
    /// By default, the Cookie saving is disabled.
    pub fn save_cookies(mut self, enabled: bool) -> Self {
        if enabled {
            self.cookies.get_or_insert_with(Default::default);
        } else {
            self.cookies.take();
        }
        self
    }

    /// Returns the value of Cookie entry with the specified name stored on this server.
    ///
    /// It returns a `None` if the specific Cookie is missing or Cookie saving is disabled.
    pub fn cookie(&self, name: &str) -> Option<&str> {
        self.cookies.as_ref()?.get(name).map(|s| s.as_str())
    }

    /// Registers a Cookie entry with the specified name and value.
    ///
    /// This method forces Cookie saving to be enabled.
    pub fn set_cookie(&mut self, name: &str, value: String) {
        self.cookies
            .get_or_insert_with(Default::default)
            .insert(name.to_owned(), value);
    }

    /// Sets the value of remote address associated with this test server.
    pub fn set_remote_addr(&mut self, addr: impl Into<RemoteAddr>) {
        self.remote_addr = addr.into();
    }

    /// Create a `Client` associated with this server.
    pub fn client(&mut self) -> crate::Result<Client<'_, S, Rt>> {
        Ok(Client {
            service: self
                .runtime
                .make_service(self.make_service.make_service(()))?,
            server: self,
        })
    }

    /// Applies a specific request to the inner service and awaits its response.
    ///
    /// This method is a shortcut to `self.client()?.perform(input)`.
    #[inline]
    pub fn perform(&mut self, input: impl Input) -> crate::Result<Response<Output>> {
        self.client()?.perform(input)
    }

    /// Waits for completing the background task spawned by the service.
    pub fn shutdown(self) {
        self.runtime.shutdown();
    }

    fn build_request(&self, input: impl Input) -> crate::Result<Request<MockRequestBody>> {
        let mut request = input.build_request()?;

        if request.extensions().get::<RemoteAddr>().is_none() {
            request.extensions_mut().insert(self.remote_addr.clone());
        }

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

/// A type that simulates an established connection with a client.
#[derive(Debug)]
pub struct Client<'a, S, Rt>
where
    S: MakeService<(), Request<MockRequestBody>>,
    Rt: Runtime<S>,
{
    service: S::Service,
    server: &'a mut Server<S, Rt>,
}

impl<'a, S, Rt> Client<'a, S, Rt>
where
    S: MakeService<(), Request<MockRequestBody>>,
    Rt: Runtime<S>,
{
    /// Applies an HTTP request to this client and await its response.
    pub fn perform(&mut self, input: impl Input) -> crate::Result<Response<Output>> {
        let request = self.server.build_request(input)?;
        let response = self.server.runtime.call(self.service.call(request))?;
        self.server.handle_set_cookies(&response)?;
        Ok(response)
    }

    /// Returns the reference to the underlying runtime.
    pub fn runtime(&mut self) -> &mut Rt {
        &mut self.server.runtime
    }
}
