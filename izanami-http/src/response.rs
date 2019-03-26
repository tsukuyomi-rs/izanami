//! HTTP response.

use {
    crate::body::Body, //
    http::{StatusCode, Uri},
};

/// Type alias of `http::Response<T>` that fixed the body type to `Body`.
pub type Response = http::Response<Body>;

/// A trait representing the conversion into an HTTP response.
pub trait IntoResponse {
    /// Converts itself into an HTTP response.
    fn into_response(self) -> Response;
}

impl IntoResponse for () {
    fn into_response(self) -> Response {
        let mut response = Response::new(Body::empty());
        *response.status_mut() = StatusCode::NO_CONTENT;
        response
    }
}

impl IntoResponse for StatusCode {
    fn into_response(self) -> Response {
        let mut response = Response::new(Body::empty());
        *response.status_mut() = self;
        response
    }
}

impl<T> IntoResponse for http::Response<T>
where
    T: Into<Body>,
{
    #[inline]
    fn into_response(self) -> Response {
        self.map(Into::into)
    }
}

impl IntoResponse for &'static str {
    #[inline]
    fn into_response(self) -> Response {
        self::make_response(self, "text/plain; charset=utf-8")
    }
}

impl IntoResponse for String {
    #[inline]
    fn into_response(self) -> Response {
        self::make_response(self, "text/plain; charset=utf-8")
    }
}

#[cfg(feature = "serde_json")]
impl IntoResponse for serde_json::Value {
    fn into_response(self) -> Response {
        let body = self.to_string();
        self::make_response(body, "application/json")
    }
}

/// Create an instance of `Response<T>` with the provided body and content type.
fn make_response<T>(body: T, content_type: &'static str) -> Response
where
    T: Into<Body>,
{
    let mut response = Response::new(body.into());
    response.headers_mut().insert(
        http::header::CONTENT_TYPE,
        http::header::HeaderValue::from_static(content_type),
    );
    response
}

/// An `IntoResponse` that represents a redirection response.
#[derive(Debug, Clone)]
pub struct Redirect {
    location: Uri,
    status: StatusCode,
}

impl Redirect {
    /// Creates a new `Redirect` with the provided URI and status code.
    pub fn new(location: Uri, status: StatusCode) -> Self {
        Self { location, status }
    }
}

impl IntoResponse for Redirect {
    #[inline]
    fn into_response(self) -> Response {
        let mut response = Response::new(Body::empty());
        *response.status_mut() = self.status;

        // TODO: optimize
        response.headers_mut().insert(
            http::header::LOCATION,
            self.location
                .to_string()
                .parse()
                .expect("should be valid header value"),
        );

        response
    }
}

macro_rules! define_redirect_constructors {
    ($(
        $(#[$doc:meta])*
        $name:ident => $STATUS:ident,
    )*) => {$(
        $(#[$doc])*
        #[inline]
        pub fn $name(location: Uri) -> Self {
            Self::new(location, StatusCode::$STATUS)
        }
    )*};
}

impl Redirect {
    define_redirect_constructors! {
        /// Create a `Redirect` with the status `301 Moved Permanently`.
        moved_permanently => MOVED_PERMANENTLY,

        /// Create a `Redirect` with the status `302 Found`.
        found => FOUND,

        /// Create a `Redirect` with the status `303 See Other`.
        see_other => SEE_OTHER,

        /// Create a `Redirect` with the status `307 Temporary Redirect`.
        temporary_redirect => TEMPORARY_REDIRECT,

        /// Create a `Redirect` with the status `308 Permanent Redirect`.
        permanent_redirect => PERMANENT_REDIRECT,
    }
}
