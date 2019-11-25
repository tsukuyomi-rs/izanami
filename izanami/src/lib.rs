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
use futures::future::{Future, TryFuture, TryFutureExt};
use http::Request;
use std::{error, pin::Pin};

type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[async_trait]
pub trait App<E: Events> {
    type Error: Into<Box<dyn error::Error + Send + Sync + 'static>>;

    async fn call(&self, req: Request<()>, events: E) -> Result<(), Self::Error>
    where
        E: 'async_trait;
}

impl<F, Fut, E> App<E> for F
where
    F: Fn(Request<()>, E) -> Fut,
    Fut: TryFuture<Ok = ()>,
    Fut::Error: Into<Box<dyn error::Error + Send + Sync + 'static>>,
    E: Events,
    F: Send + Sync,
    Fut: Send,
    E: Send,
{
    type Error = Fut::Error;

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
        Box::pin(async move { (*self)(req, events).into_future().await })
    }
}

impl<T: ?Sized, E> App<E> for std::sync::Arc<T>
where
    T: App<E>,
    E: Events,
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

// for reservation.
pub trait Events {}
