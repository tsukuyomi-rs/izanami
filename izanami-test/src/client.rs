use {
    crate::{
        output::Output, //
        runtime::Runtime,
        server::Server,
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

#[derive(Debug)]
pub struct Builder<'a, S, Rt>
where
    S: MakeTestService,
    Rt: Runtime<S>,
{
    server: &'a mut Server<S, Rt>,
    remote_addr: Option<RemoteAddr>,
}

impl<'a, S, Rt> Builder<'a, S, Rt>
where
    S: MakeTestService,
    Rt: Runtime<S>,
{
    pub(crate) fn new(server: &'a mut Server<S, Rt>) -> Self {
        Self {
            server,
            remote_addr: None,
        }
    }

    /// Sets the value of remote address associated with the client.
    pub fn remote_addr(self, remote_addr: impl Into<RemoteAddr>) -> Self {
        Self {
            remote_addr: Some(remote_addr.into()),
            ..self
        }
    }

    /// Consume itself and creates an instance of `Client` using the current configuration.
    pub fn build(self) -> crate::Result<Client<'a, S, Rt>> {
        let remote_addr = self
            .remote_addr
            .unwrap_or_else(|| RemoteAddr::tcp(([127, 0, 0, 1], 12345).into()));

        let service = self.server.runtime.make_service(
            self.server
                .make_service
                .make_service(crate::service::TestContext::new()),
        )?;

        Ok(Client {
            service,
            server: self.server,
            cookies: None,
            remote_addr,
        })
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
    cookies: Option<HashMap<String, String>>,
    remote_addr: RemoteAddr,
}

impl<'a, S, Rt> Client<'a, S, Rt>
where
    S: MakeTestService,
    Rt: Runtime<S>,
{
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

    /// Returns a reference to the remote address associated with this client.
    pub fn remote_addr(&self) -> &RemoteAddr {
        &self.remote_addr
    }

    /// Applies an HTTP request to this client and await its response.
    pub fn request<Bd>(
        &mut self,
        request: Request<Bd>,
    ) -> crate::Result<AwaitResponse<'_, 'a, S, Rt>>
    where
        Bd: Into<MockRequestBody>,
    {
        let mut request = request.map(Into::into);
        self.prepare_request(&mut request)?;

        let response = self.server.runtime.call(self.service.call(request))?;
        self.after_response(&response)?;

        Ok(AwaitResponse {
            response,
            client: self,
        })
    }

    fn prepare_request<T>(&self, request: &mut Request<T>) -> crate::Result<()> {
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

        Ok(())
    }

    fn after_response<T>(&mut self, response: &Response<T>) -> crate::Result<()> {
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

#[allow(missing_debug_implementations)]
pub struct AwaitResponse<'c, 's, S, Rt>
where
    S: MakeTestService,
    Rt: Runtime<S>,
{
    response: Response<S::ResponseBody>,
    client: &'c mut Client<'s, S, Rt>,
}

impl<'c, 's, S, Rt> std::ops::Deref for AwaitResponse<'c, 's, S, Rt>
where
    S: MakeTestService,
    Rt: Runtime<S>,
{
    type Target = Response<S::ResponseBody>;

    fn deref(&self) -> &Self::Target {
        &self.response
    }
}

impl<'c, 's, S, Rt> std::ops::DerefMut for AwaitResponse<'c, 's, S, Rt>
where
    S: MakeTestService,
    Rt: Runtime<S>,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.response
    }
}

impl<'c, 's, S, Rt> AwaitResponse<'c, 's, S, Rt>
where
    S: MakeTestService,
    Rt: Runtime<S>,
{
    pub fn into_response(self) -> Response<S::ResponseBody> {
        self.response
    }

    pub fn send(self) -> crate::Result<Output> {
        self.client
            .server
            .runtime
            .receive_body(self.response.into_body())
    }
}
