use crate::events::Events;
use async_trait::async_trait;
use http::Request;
use std::{future::Future, pin::Pin};

#[async_trait]
pub trait App {
    async fn call<E>(&self, req: &Request<()>, events: E) -> anyhow::Result<()>
    where
        E: Events;
}

impl<'a, T: ?Sized> App for &'a T
where
    T: App,
{
    #[inline]
    fn call<'l1, 'l2, 'async_trait, E>(
        &'l1 self,
        req: &'l2 Request<()>,
        events: E,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send + 'async_trait>>
    where
        'l1: 'async_trait,
        'l2: 'async_trait,
        E: Events + 'async_trait,
    {
        (**self).call(req, events)
    }
}

impl<T: ?Sized> App for Box<T>
where
    T: App,
{
    #[inline]
    fn call<'l1, 'l2, 'async_trait, E>(
        &'l1 self,
        req: &'l2 Request<()>,
        events: E,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send + 'async_trait>>
    where
        'l1: 'async_trait,
        'l2: 'async_trait,
        E: Events + 'async_trait,
    {
        (**self).call(req, events)
    }
}

impl<T: ?Sized> App for std::sync::Arc<T>
where
    T: App,
{
    #[inline]
    fn call<'l1, 'l2, 'async_trait, E>(
        &'l1 self,
        req: &'l2 Request<()>,
        events: E,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send + 'async_trait>>
    where
        'l1: 'async_trait,
        'l2: 'async_trait,
        E: Events + 'async_trait,
    {
        (**self).call(req, events)
    }
}
