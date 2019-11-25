//! Web application interface inspired from ASGI.

#![doc(html_root_url = "https://docs.rs/izanami/0.2.0-dev")]
#![deny(
    missing_debug_implementations,
    nonstandard_style,
    rust_2018_idioms,
    rust_2018_compatibility,
    unused
)]
#![forbid(clippy::unimplemented)]
#![cfg_attr(test, deny(warnings))]

use async_trait::async_trait;
use http::Request;

type BoxFuture<'a, T> = std::pin::Pin<Box<dyn std::future::Future<Output = T> + Send + 'a>>;

#[async_trait]
pub trait App<E> {
    type Error: Into<Box<dyn std::error::Error + Send + Sync + 'static>>;

    async fn call(&self, req: Request<()>, events: E) -> Result<(), Self::Error>
    where
        E: 'async_trait;
}

impl<'a, T: ?Sized, E> App<E> for &'a T
where
    T: App<E>,
{
    type Error = T::Error;

    #[inline]
    fn call<'l1, 'async_trait>(
        &'l1 self,
        req: Request<()>,
        events: E,
    ) -> BoxFuture<'async_trait, Result<(), Self::Error>>
    where
        'l1: 'async_trait,
        E: 'async_trait,
    {
        (**self).call(req, events)
    }
}

impl<T: ?Sized, E> App<E> for Box<T>
where
    T: App<E>,
{
    type Error = T::Error;

    #[inline]
    fn call<'l1, 'async_trait>(
        &'l1 self,
        req: Request<()>,
        events: E,
    ) -> BoxFuture<'async_trait, Result<(), Self::Error>>
    where
        'l1: 'async_trait,
        E: 'async_trait,
    {
        (**self).call(req, events)
    }
}

impl<T: ?Sized, E> App<E> for std::sync::Arc<T>
where
    T: App<E>,
{
    type Error = T::Error;

    #[inline]
    fn call<'l1, 'async_trait>(
        &'l1 self,
        req: Request<()>,
        events: E,
    ) -> BoxFuture<'async_trait, Result<(), Self::Error>>
    where
        'l1: 'async_trait,
        E: 'async_trait,
    {
        (**self).call(req, events)
    }
}
