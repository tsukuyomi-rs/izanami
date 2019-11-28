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
use bytes::Buf;
use http::{HeaderMap, Request, Response};
use std::{error, future::Future, pin::Pin};

type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// A trait that models Web applications.
///
/// Compared to the traditional request-response model, it has
/// the following characteristics:
///
/// * Receiving data from the client and sending the response
///   header/body is performed via a single instance that implements
///   `Events` provided by the server.
///
/// * The response is sent via the callback function provided to `events`
///   not via the return value of function. The application is able
///   to continue any processing after sending the response to the client.
///
/// * There is no need to expose the concrete (or abstract) type of response
///   body as an associated type in the trait.
///
/// * (Bidirectional) streaming is performed by calling the callback function
///   provided to `events`.
#[async_trait]
pub trait App<E: Events> {
    /// The error type returned from `call`.
    ///
    /// The error value is typically used to notify the server that a fatal
    /// error has occurred and any kind of messages cannot be sent to the
    /// client. If the server receives this error, it should interrupt the
    /// processing of the application for that request and send an appropriate
    /// signal to the client or close the connection.
    ///
    /// This error cannot be used for the purpose to send an error to the
    /// client. The application should send a response with the appropriate
    /// error code on error.
    type Error: Into<Box<dyn error::Error + Send + Sync + 'static>>;

    /// Handle an incoming HTTP request.
    async fn call(&self, req: Request<E>) -> Result<(), Self::Error>
    where
        E: 'async_trait;
}

impl<'a, T: ?Sized, E> App<E> for &'a T
where
    T: App<E>,
    E: Events,
{
    type Error = T::Error;

    #[inline]
    fn call<'l1, 'async_trait>(
        &'l1 self,
        request: Request<E>,
    ) -> BoxFuture<'async_trait, Result<(), Self::Error>>
    where
        'l1: 'async_trait,
        E: 'async_trait,
    {
        (**self).call(request)
    }
}

impl<T: ?Sized, E> App<E> for Box<T>
where
    T: App<E>,
    E: Events,
{
    type Error = T::Error;

    #[inline]
    fn call<'l1, 'async_trait>(
        &'l1 self,
        request: Request<E>,
    ) -> BoxFuture<'async_trait, Result<(), Self::Error>>
    where
        'l1: 'async_trait,
        E: 'async_trait,
    {
        (**self).call(request)
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
        request: Request<E>,
    ) -> BoxFuture<'async_trait, Result<(), Self::Error>>
    where
        'l1: 'async_trait,
        E: 'async_trait,
    {
        (**self).call(request)
    }
}

/// Asynchronous object that exchanges the events with the client.
#[async_trait]
pub trait Events {
    type Data: Buf;
    type Error: Into<Box<dyn error::Error + Send + Sync + 'static>>;

    async fn data(&mut self) -> Option<Result<Self::Data, Self::Error>>;

    async fn trailers(&mut self) -> Result<Option<HeaderMap>, Self::Error>;

    async fn start_send_response(
        &mut self,
        response: Response<()>,
        end_of_stream: bool,
    ) -> Result<(), Self::Error>;

    async fn send_data(&mut self, data: Self::Data, end_of_stream: bool)
        -> Result<(), Self::Error>;

    async fn send_trailers(&mut self, trailers: HeaderMap) -> Result<(), Self::Error>;
}

impl<'a, E: ?Sized> Events for &'a mut E
where
    E: Events,
{
    type Data = E::Data;
    type Error = E::Error;

    #[inline]
    fn data<'l1, 'async_trait>(
        &'l1 mut self,
    ) -> BoxFuture<'async_trait, Option<Result<Self::Data, Self::Error>>>
    where
        'l1: 'async_trait,
    {
        (**self).data()
    }

    #[inline]
    fn trailers<'l1, 'async_trait>(
        &'l1 mut self,
    ) -> BoxFuture<'async_trait, Result<Option<HeaderMap>, Self::Error>>
    where
        'l1: 'async_trait,
    {
        (**self).trailers()
    }

    #[inline]
    fn start_send_response<'l1, 'async_trait>(
        &'l1 mut self,
        response: Response<()>,
        end_of_stream: bool,
    ) -> BoxFuture<'async_trait, Result<(), Self::Error>>
    where
        'l1: 'async_trait,
    {
        (**self).start_send_response(response, end_of_stream)
    }

    #[inline]
    fn send_data<'l1, 'async_trait>(
        &'l1 mut self,
        data: Self::Data,
        end_of_stream: bool,
    ) -> BoxFuture<'async_trait, Result<(), Self::Error>>
    where
        'l1: 'async_trait,
    {
        (**self).send_data(data, end_of_stream)
    }

    #[inline]
    fn send_trailers<'l1, 'async_trait>(
        &'l1 mut self,
        trailers: HeaderMap,
    ) -> BoxFuture<'async_trait, Result<(), Self::Error>>
    where
        'l1: 'async_trait,
    {
        (**self).send_trailers(trailers)
    }
}

impl<E: ?Sized> Events for Box<E>
where
    E: Events,
{
    type Data = E::Data;
    type Error = E::Error;

    #[inline]
    fn data<'l1, 'async_trait>(
        &'l1 mut self,
    ) -> BoxFuture<'async_trait, Option<Result<Self::Data, Self::Error>>>
    where
        'l1: 'async_trait,
    {
        (**self).data()
    }

    #[inline]
    fn trailers<'l1, 'async_trait>(
        &'l1 mut self,
    ) -> BoxFuture<'async_trait, Result<Option<HeaderMap>, Self::Error>>
    where
        'l1: 'async_trait,
    {
        (**self).trailers()
    }

    #[inline]
    fn start_send_response<'l1, 'async_trait>(
        &'l1 mut self,
        response: Response<()>,
        end_of_stream: bool,
    ) -> BoxFuture<'async_trait, Result<(), Self::Error>>
    where
        'l1: 'async_trait,
    {
        (**self).start_send_response(response, end_of_stream)
    }

    #[inline]
    fn send_data<'l1, 'async_trait>(
        &'l1 mut self,
        data: Self::Data,
        end_of_stream: bool,
    ) -> BoxFuture<'async_trait, Result<(), Self::Error>>
    where
        'l1: 'async_trait,
    {
        (**self).send_data(data, end_of_stream)
    }

    #[inline]
    fn send_trailers<'l1, 'async_trait>(
        &'l1 mut self,
        trailers: HeaderMap,
    ) -> BoxFuture<'async_trait, Result<(), Self::Error>>
    where
        'l1: 'async_trait,
    {
        (**self).send_trailers(trailers)
    }
}
