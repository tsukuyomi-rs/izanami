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
