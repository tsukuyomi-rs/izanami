use {
    crate::{body::HttpBody, error::Error, handler::Handler},
    std::{rc::Rc, sync::Arc},
};

pub trait Middleware<H: Handler> {
    type Body: HttpBody;
    type Error: Into<Error>;
    type Handler: Handler<Body = Self::Body, Error = Self::Error>;

    fn wrap(&self, inner: H) -> Self::Handler;
}

impl<'a, M, H> Middleware<H> for &'a M
where
    M: Middleware<H> + ?Sized,
    H: Handler,
{
    type Body = M::Body;
    type Error = M::Error;
    type Handler = M::Handler;

    fn wrap(&self, inner: H) -> Self::Handler {
        (**self).wrap(inner)
    }
}

impl<M, H> Middleware<H> for Box<M>
where
    M: Middleware<H> + ?Sized,
    H: Handler,
{
    type Body = M::Body;
    type Error = M::Error;
    type Handler = M::Handler;

    fn wrap(&self, inner: H) -> Self::Handler {
        (**self).wrap(inner)
    }
}

impl<M, H> Middleware<H> for Rc<M>
where
    M: Middleware<H> + ?Sized,
    H: Handler,
{
    type Body = M::Body;
    type Error = M::Error;
    type Handler = M::Handler;

    fn wrap(&self, inner: H) -> Self::Handler {
        (**self).wrap(inner)
    }
}

impl<M, H> Middleware<H> for Arc<M>
where
    M: Middleware<H> + ?Sized,
    H: Handler,
{
    type Body = M::Body;
    type Error = M::Error;
    type Handler = M::Handler;

    fn wrap(&self, inner: H) -> Self::Handler {
        (**self).wrap(inner)
    }
}
