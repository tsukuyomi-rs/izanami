use crate::Service;

/// A trait to be converted into an asynchronous function.
pub trait IntoService<T, Request> {
    /// The response type of `Service`.
    type Response;

    /// The error type of `Service`.
    type Error;

    /// The type of `Service` obtained by converting the value of this type.
    type Service: Service<Request, Response = Self::Response, Error = Self::Error>;

    /// Cosumes itself and convert it into a `Service`.
    fn into_service(self, ctx: T) -> Self::Service;
}

impl<S, T, Req> IntoService<T, Req> for S
where
    S: Service<Req>,
{
    type Response = S::Response;
    type Error = S::Error;
    type Service = S;

    fn into_service(self, _: T) -> Self::Service {
        self
    }
}
