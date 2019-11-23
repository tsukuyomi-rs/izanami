use crate::eventer::Eventer;
use async_trait::async_trait;
use http::Request;
use std::{future::Future, io, pin::Pin};

#[async_trait]
pub trait App {
    async fn call<E>(&self, req: &Request<()>, ev: E) -> io::Result<()>
    where
        E: Eventer;
}

impl<'a, T: ?Sized> App for &'a T
where
    T: App,
{
    fn call<'l1, 'l2, 'async_trait, E>(
        &'l1 self,
        req: &'l2 Request<()>,
        ev: E,
    ) -> Pin<Box<dyn Future<Output = io::Result<()>> + Send + 'async_trait>>
    where
        'l1: 'async_trait,
        'l2: 'async_trait,
        E: Eventer + 'async_trait,
    {
        (**self).call(req, ev)
    }
}

impl<T: ?Sized> App for Box<T>
where
    T: App,
{
    fn call<'l1, 'l2, 'async_trait, E>(
        &'l1 self,
        req: &'l2 Request<()>,
        ev: E,
    ) -> Pin<Box<dyn Future<Output = io::Result<()>> + Send + 'async_trait>>
    where
        'l1: 'async_trait,
        'l2: 'async_trait,
        E: Eventer + 'async_trait,
    {
        (**self).call(req, ev)
    }
}

impl<T: ?Sized> App for std::sync::Arc<T>
where
    T: App,
{
    fn call<'l1, 'l2, 'async_trait, E>(
        &'l1 self,
        req: &'l2 Request<()>,
        ev: E,
    ) -> Pin<Box<dyn Future<Output = io::Result<()>> + Send + 'async_trait>>
    where
        'l1: 'async_trait,
        'l2: 'async_trait,
        E: Eventer + 'async_trait,
    {
        (**self).call(req, ev)
    }
}
