use crate::{events::Events, BoxFuture};
use async_trait::async_trait;
use http::Request;

#[async_trait]
pub trait App<E>
where
    E: Events,
{
    async fn call(&self, req: &Request<()>, events: E) -> anyhow::Result<()>
    where
        E: 'async_trait;
}

impl<'a, T: ?Sized, E> App<E> for &'a T
where
    T: App<E>,
    E: Events,
{
    #[inline]
    fn call<'l1, 'l2, 'async_trait>(
        &'l1 self,
        req: &'l2 Request<()>,
        events: E,
    ) -> BoxFuture<'async_trait, anyhow::Result<()>>
    where
        'l1: 'async_trait,
        'l2: 'async_trait,
        E: 'async_trait,
    {
        (**self).call(req, events)
    }
}

impl<T: ?Sized, E> App<E> for Box<T>
where
    T: App<E>,
    E: Events,
{
    #[inline]
    fn call<'l1, 'l2, 'async_trait>(
        &'l1 self,
        req: &'l2 Request<()>,
        events: E,
    ) -> BoxFuture<'async_trait, anyhow::Result<()>>
    where
        'l1: 'async_trait,
        'l2: 'async_trait,
        E: 'async_trait,
    {
        (**self).call(req, events)
    }
}

impl<T: ?Sized, E> App<E> for std::sync::Arc<T>
where
    T: App<E>,
    E: Events,
{
    #[inline]
    fn call<'l1, 'l2, 'async_trait>(
        &'l1 self,
        req: &'l2 Request<()>,
        events: E,
    ) -> BoxFuture<'async_trait, anyhow::Result<()>>
    where
        'l1: 'async_trait,
        'l2: 'async_trait,
        E: 'async_trait,
    {
        (**self).call(req, events)
    }
}
