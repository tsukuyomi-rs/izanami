//! Definition of `Handler`.

use {
    crate::{body::HttpBody, context::Context, error::Error},
    futures::{Future, Poll},
    http::Response,
};

/// Asynchronous HTTP handler dispatched per incoming requests.
pub trait Handler {
    type Body: HttpBody;
    type Error: Into<Error>;

    /// Polls the response to the client with the specified context.
    fn poll_http(&mut self, cx: &mut Context<'_>) -> Poll<Response<Self::Body>, Self::Error>;
}

impl<F, B> Handler for F
where
    F: Future<Item = Response<B>>,
    F::Error: Into<Error>,
    B: HttpBody,
{
    type Body = B;
    type Error = F::Error;

    fn poll_http(&mut self, cx: &mut Context<'_>) -> Poll<Response<Self::Body>, Self::Error> {
        crate::context::set_tls(cx, || Future::poll(self))
    }
}
