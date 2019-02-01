use {
    crate::{
        output::Output, //
        runtime::Runtime,
        service::{MakeTestService, MockRequestBody, TestService},
    },
    cookie::Cookie,
    http::{
        header::{COOKIE, SET_COOKIE},
        Request, Response,
    },
    izanami_util::RemoteAddr,
    std::collections::HashMap,
};

/// A type that simulates an HTTP server without using the low-level I/O.
#[derive(Debug)]
pub struct Server<S, Rt = tokio::runtime::Runtime>
where
    S: MakeTestService,
    Rt: Runtime<S>,
{
    make_service: S,
    cookies: Option<HashMap<String, String>>,
    remote_addr: RemoteAddr,
    runtime: Rt,
}

impl<S, Rt> Server<S, Rt>
where
    S: MakeTestService,
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
            service: self.runtime.make_service(
                self.make_service
                    .make_service(crate::service::TestContext::new()),
            )?,
            server: self,
        })
    }

    /// Applies a specific request to the inner service and awaits its response.
    ///
    /// This method is a shortcut to `self.client()?.perform(input)`.
    #[inline]
    pub fn perform(
        &mut self,
        request: Request<impl Into<MockRequestBody>>,
    ) -> crate::Result<Response<Output>> {
        self.client()?.perform(request)
    }

    /// Waits for completing the background task spawned by the service.
    pub fn shutdown(self) {
        self.runtime.shutdown();
    }

    fn build_request(
        &self,
        request: Request<impl Into<MockRequestBody>>,
    ) -> crate::Result<Request<MockRequestBody>> {
        let mut request = request.map(Into::into);

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
    S: MakeTestService,
    Rt: Runtime<S>,
{
    service: S::Service,
    server: &'a mut Server<S, Rt>,
}

impl<'a, S, Rt> Client<'a, S, Rt>
where
    S: MakeTestService,
    Rt: Runtime<S>,
{
    /// Applies an HTTP request to this client and await its response.
    pub fn perform(
        &mut self,
        request: Request<impl Into<MockRequestBody>>,
    ) -> crate::Result<Response<Output>> {
        let request = self.server.build_request(request)?;
        let response = self.server.runtime.call(self.service.call(request))?;
        self.server.handle_set_cookies(&response)?;
        Ok(response)
    }

    /// Returns the reference to the underlying runtime.
    pub fn runtime(&mut self) -> &mut Rt {
        &mut self.server.runtime
    }
}
