use {
    crate::{
        runtime::Runtime,
        server::Server,
        service::{MakeTestService, MockRequestBody, ResponseBody, TestService},
    },
    bytes::{Buf, Bytes},
    cookie::Cookie,
    futures::{Async, Future, Poll},
    http::{
        header::{COOKIE, SET_COOKIE},
        Request, Response,
    },
    izanami_util::RemoteAddr,
    std::collections::HashMap,
    std::{borrow::Cow, str},
};

/// A type that simulates an established connection with a client.
#[derive(Debug)]
pub struct Client<'a, S, Rt>
where
    S: MakeTestService,
    Rt: Runtime<S>,
{
    service: S::Service,
    server: &'a mut Server<S, Rt>,
    cookies: HashMap<String, String>,
}

impl<'a, S, Rt> Client<'a, S, Rt>
where
    S: MakeTestService,
    Rt: Runtime<S>,
{
    pub(crate) fn new(server: &'a mut Server<S, Rt>, service: S::Service) -> Self {
        Client {
            server,
            service,
            cookies: HashMap::new(),
        }
    }

    /// Returns the value of Cookie entry with the specified name stored on this server.
    ///
    /// It returns a `None` if the specific Cookie is missing or Cookie saving is disabled.
    pub fn cookie(&self, name: &str) -> Option<&str> {
        self.cookies.get(name).map(|s| s.as_str())
    }

    /// Registers a Cookie entry with the specified name and value.
    pub fn set_cookie(&mut self, name: &str, value: String) {
        if self.server.cookie_enabled() {
            self.cookies.insert(name.to_owned(), value);
        }
    }

    /// Applies an HTTP request to this client and await its response.
    pub fn respond<Bd>(
        &mut self,
        request: Request<Bd>,
    ) -> crate::Result<AwaitResponse<'_, 'a, S, Rt>>
    where
        Bd: Into<MockRequestBody>,
    {
        let mut request = request.map(Into::into);
        self.prepare_request(&mut request)?;

        let response = {
            let (_, runtime) = self.server.get_mut();
            runtime.call(self.service.call(request))?
        };
        self.after_respond(&response)?;

        Ok(AwaitResponse {
            response,
            client: self,
        })
    }

    fn prepare_request<T>(&self, request: &mut Request<T>) -> crate::Result<()> {
        if request.extensions().get::<RemoteAddr>().is_none() {
            request
                .extensions_mut()
                .insert(self.server.remote_addr().clone());
        }

        if self.server.cookie_enabled() {
            for (k, v) in &self.cookies {
                let cookie = Cookie::new(k.to_owned(), v.to_owned())
                    .to_string()
                    .parse()?;
                request.headers_mut().append(COOKIE, cookie);
            }
        }

        Ok(())
    }

    fn after_respond<T>(&mut self, response: &Response<T>) -> crate::Result<()> {
        if self.server.cookie_enabled() {
            for set_cookie in response.headers().get_all(SET_COOKIE) {
                let cookie = Cookie::parse_encoded(set_cookie.to_str()?)?;
                if cookie.value().is_empty() {
                    self.cookies.remove(cookie.name());
                } else {
                    self.cookies
                        .insert(cookie.name().to_owned(), cookie.value().to_owned());
                }
            }
        }

        Ok(())
    }
}

/// A type representing the result when the `Future`
/// returned from `S::Service` is completed.
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

    /// Converts the internal response body into a `Future` and awaits its result.
    pub fn send_body(self) -> crate::Result<ResponseData> {
        let send_body = SendResponseBody {
            state: SendResponseBodyState::Init(Some(self.response.into_body())),
        };
        let (_, runtime) = self.client.server.get_mut();
        runtime.send_response_body(send_body)
    }
}

#[doc(hidden)]
#[allow(missing_debug_implementations)]
pub struct SendResponseBody<Bd> {
    state: SendResponseBodyState<Bd>,
}

#[allow(missing_debug_implementations)]
enum SendResponseBodyState<Bd> {
    Init(Option<Bd>),
    InFlight { body: Bd, chunks: Vec<Bytes> },
}

impl<Bd> Future for SendResponseBody<Bd>
where
    Bd: ResponseBody,
{
    type Item = ResponseData;
    type Error = Bd::Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        loop {
            self.state = match self.state {
                SendResponseBodyState::Init(ref mut body) => SendResponseBodyState::InFlight {
                    body: body.take().expect("unexpected condition"),
                    chunks: vec![],
                },
                SendResponseBodyState::InFlight {
                    ref mut body,
                    ref mut chunks,
                } => {
                    while let Some(chunk) = futures::try_ready!(body.poll_buf()) {
                        chunks.push(chunk.collect());
                    }
                    return Ok(Async::Ready(ResponseData {
                        chunks: std::mem::replace(chunks, vec![]),
                        _priv: (),
                    }));
                }
            }
        }
    }
}

/// A collection of data generated by the response body.
#[derive(Debug)]
pub struct ResponseData {
    pub chunks: Vec<Bytes>,
    _priv: (),
}

impl ResponseData {
    /// Returns a representation of the chunks as a byte sequence.
    pub fn to_bytes(&self) -> Cow<'_, [u8]> {
        match self.chunks.len() {
            0 => Cow::Borrowed(&[]),
            1 => Cow::Borrowed(&self.chunks[0]),
            _ => Cow::Owned(self.chunks.iter().fold(Vec::new(), |mut acc, chunk| {
                acc.extend_from_slice(&*chunk);
                acc
            })),
        }
    }

    /// Returns a representation of the chunks as an UTF-8 sequence.
    pub fn to_utf8(&self) -> Result<Cow<'_, str>, str::Utf8Error> {
        match self.to_bytes() {
            Cow::Borrowed(bytes) => str::from_utf8(bytes).map(Cow::Borrowed),
            Cow::Owned(bytes) => String::from_utf8(bytes)
                .map_err(|e| e.utf8_error())
                .map(Cow::Owned),
        }
    }
}
