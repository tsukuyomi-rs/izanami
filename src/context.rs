use {
    crate::{body::Body, localmap::LocalMap},
    cookie2::{Cookie, CookieJar},
    http::{HeaderMap, Request},
    std::{cell::Cell, error, fmt, mem, ptr::NonNull, str},
};

thread_local! {
    static TLS_CX: Cell<Option<NonNull<Context<'static>>>> = Cell::new(None);
}

#[allow(missing_debug_implementations)]
struct SetOnDrop(Option<NonNull<Context<'static>>>);

impl Drop for SetOnDrop {
    fn drop(&mut self) {
        TLS_CX.with(|cx| {
            cx.set(self.0.take());
        })
    }
}

/// Set a reference to the request context to task local storage.
pub fn set_tls_context<F, R>(cx: &mut Context<'_>, f: F) -> R
where
    F: FnOnce() -> R,
{
    let cx: &mut Context<'static> = unsafe { mem::transmute(cx) };
    let old_cx = TLS_CX.with(|tls_cx| tls_cx.replace(Some(NonNull::from(cx))));
    let _reset = SetOnDrop(old_cx);
    f()
}

/// Retrieve a reference to the request context from task local storage.
///
/// # Panics
///
/// This function will panic if the request context is not set at the current task.
pub fn get_tls_context<F, R>(f: F) -> R
where
    F: FnOnce(&mut Context<'_>) -> R,
{
    let cx_ptr = TLS_CX.with(|tls_cx| tls_cx.replace(None));
    let _reset = SetOnDrop(cx_ptr);
    let mut cx_ptr = cx_ptr.expect("The request context is not set at the current task");
    unsafe { f(cx_ptr.as_mut()) }
}

#[derive(Debug)]
pub(crate) struct ContextInner {
    pub(crate) request: Request<()>,
    pub(crate) body: Option<Body>,
    pub(crate) cookies: Option<CookieJar>,
    pub(crate) response_headers: Option<HeaderMap>,
    pub(crate) locals: LocalMap,
}

impl ContextInner {
    pub(crate) fn new(request: Request<Body>) -> Self {
        let (parts, body) = request.into_parts();
        Self {
            request: Request::from_parts(parts, ()),
            body: Some(body),
            cookies: None,
            response_headers: None,
            locals: LocalMap::default(),
        }
    }
}

/// A set of context values associated with an incoming request.
#[derive(Debug)]
pub struct Context<'a>(pub(crate) &'a mut ContextInner);

impl<'a> Context<'a> {
    pub fn protocol_version(&self) -> http::Version {
        self.0.request.version()
    }

    pub fn method(&self) -> &http::Method {
        self.0.request.method()
    }

    pub fn uri(&self) -> &http::Uri {
        self.0.request.uri()
    }

    pub fn path(&self) -> &str {
        self.uri().path()
    }

    pub fn query(&self) -> Option<&str> {
        self.uri().query()
    }

    pub fn headers(&self) -> &http::HeaderMap {
        self.0.request.headers()
    }

    pub fn extensions(&self) -> &http::Extensions {
        self.0.request.extensions()
    }

    pub fn body(&mut self) -> Option<Body> {
        self.0.body.take()
    }

    pub fn cookies(&mut self) -> Result<&mut CookieJar, CookieParseError> {
        if let Some(ref mut jar) = self.0.cookies {
            return Ok(jar);
        }

        let mut jar = CookieJar::new();

        for hdr in self.headers().get_all(http::header::COOKIE) {
            let hdr = str::from_utf8(hdr.as_bytes()).map_err(CookieParseError::new)?;
            for s in hdr.split(';').map(str::trim) {
                let cookie = Cookie::parse_encoded(s)
                    .map_err(CookieParseError::new)?
                    .into_owned();
                jar.add_original(cookie);
            }
        }

        Ok(self.0.cookies.get_or_insert(jar))
    }

    pub fn response_headers(&mut self) -> &mut HeaderMap {
        self.0.response_headers.get_or_insert_with(Default::default)
    }

    pub fn locals(&self) -> &LocalMap {
        &self.0.locals
    }

    pub fn locals_mut(&mut self) -> &mut LocalMap {
        &mut self.0.locals
    }
}

#[derive(Debug)]
pub struct CookieParseError(Option<Box<dyn error::Error + Send + Sync>>);

impl CookieParseError {
    fn new<E>(err: E) -> Self
    where
        E: Into<Box<dyn error::Error + Send + Sync>>,
    {
        CookieParseError(Some(err.into()))
    }
}

impl fmt::Display for CookieParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("failed to parse Cookie values")
    }
}

impl error::Error for CookieParseError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        self.0
            .as_ref()
            .map(|e| &**e as &(dyn error::Error + 'static))
    }
}
