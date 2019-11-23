use async_trait::async_trait;
use bytes::Buf;
use http::{HeaderMap, Response};
use std::{error::Error, future::Future, pin::Pin};

#[async_trait]
pub trait Eventer: Send {
    type Data: Buf + Send;
    type Error: Into<Box<dyn Error + Send + Sync>>;

    async fn data(&mut self) -> Result<Option<Self::Data>, Self::Error>;
    async fn trailers(&mut self) -> Result<Option<HeaderMap>, Self::Error>;

    async fn send_response<T>(&mut self, response: Response<T>) -> Result<(), Self::Error>
    where
        T: Buf + Send,
    {
        let (parts, body) = response.into_parts();
        self.start_send_response(Response::from_parts(parts, ()))
            .await?;
        self.send_data(body, true).await?;
        Ok(())
    }

    async fn start_send_response(&mut self, response: Response<()>) -> Result<(), Self::Error>;
    async fn send_data<T>(&mut self, data: T, end_of_stream: bool) -> Result<(), Self::Error>
    where
        T: Buf + Send;
    async fn send_trailers(&mut self, trailers: HeaderMap) -> Result<(), Self::Error>;
}

impl<E: ?Sized> Eventer for &mut E
where
    E: Eventer,
{
    type Data = E::Data;
    type Error = E::Error;

    fn data<'l1, 'async_trait>(
        &'l1 mut self,
    ) -> crate::BoxFuture<'async_trait, Result<Option<Self::Data>, Self::Error>>
    where
        'l1: 'async_trait,
        Self: 'async_trait,
        Self::Data: 'async_trait,
        Self::Error: 'async_trait,
    {
        (**self).data()
    }

    fn trailers<'l1, 'async_trait>(
        &'l1 mut self,
    ) -> crate::BoxFuture<'async_trait, Result<Option<HeaderMap>, Self::Error>>
    where
        'l1: 'async_trait,
        Self: 'async_trait,
        Self::Data: 'async_trait,
        Self::Error: 'async_trait,
    {
        (**self).trailers()
    }

    fn send_response<'l1, 'async_trait, T>(
        &'l1 mut self,
        response: Response<T>,
    ) -> Pin<Box<dyn Future<Output = Result<(), Self::Error>> + Send + 'async_trait>>
    where
        T: Buf + Send + 'async_trait,
        'l1: 'async_trait,
        Self: 'async_trait,
    {
        (**self).send_response(response)
    }

    fn start_send_response<'l1, 'async_trait>(
        &'l1 mut self,
        response: Response<()>,
    ) -> Pin<Box<dyn Future<Output = Result<(), Self::Error>> + Send + 'async_trait>>
    where
        'l1: 'async_trait,
        Self: 'async_trait,
    {
        (**self).start_send_response(response)
    }

    fn send_data<'l1, 'async_trait, T>(
        &'l1 mut self,
        data: T,
        end_of_stream: bool,
    ) -> Pin<Box<dyn Future<Output = Result<(), Self::Error>> + Send + 'async_trait>>
    where
        T: Buf + Send + 'async_trait,
        'l1: 'async_trait,
        Self: 'async_trait,
    {
        (**self).send_data(data, end_of_stream)
    }

    fn send_trailers<'l1, 'async_trait>(
        &'l1 mut self,
        trailers: HeaderMap,
    ) -> Pin<Box<dyn Future<Output = Result<(), Self::Error>> + Send + 'async_trait>>
    where
        'l1: 'async_trait,
        Self: 'async_trait,
    {
        (**self).send_trailers(trailers)
    }
}

impl<E: ?Sized> Eventer for Box<E>
where
    E: Eventer,
{
    type Data = E::Data;
    type Error = E::Error;

    fn data<'l1, 'async_trait>(
        &'l1 mut self,
    ) -> crate::BoxFuture<'async_trait, Result<Option<Self::Data>, Self::Error>>
    where
        'l1: 'async_trait,
        Self: 'async_trait,
        Self::Data: 'async_trait,
        Self::Error: 'async_trait,
    {
        (**self).data()
    }

    fn trailers<'l1, 'async_trait>(
        &'l1 mut self,
    ) -> crate::BoxFuture<'async_trait, Result<Option<HeaderMap>, Self::Error>>
    where
        'l1: 'async_trait,
        Self: 'async_trait,
        Self::Data: 'async_trait,
        Self::Error: 'async_trait,
    {
        (**self).trailers()
    }

    fn send_response<'l1, 'async_trait, T>(
        &'l1 mut self,
        response: Response<T>,
    ) -> Pin<Box<dyn Future<Output = Result<(), Self::Error>> + Send + 'async_trait>>
    where
        T: Buf + Send + 'async_trait,
        'l1: 'async_trait,
        Self: 'async_trait,
    {
        (**self).send_response(response)
    }

    fn start_send_response<'l1, 'async_trait>(
        &'l1 mut self,
        response: Response<()>,
    ) -> Pin<Box<dyn Future<Output = Result<(), Self::Error>> + Send + 'async_trait>>
    where
        'l1: 'async_trait,
        Self: 'async_trait,
    {
        (**self).start_send_response(response)
    }

    fn send_data<'l1, 'async_trait, T>(
        &'l1 mut self,
        data: T,
        end_of_stream: bool,
    ) -> Pin<Box<dyn Future<Output = Result<(), Self::Error>> + Send + 'async_trait>>
    where
        T: Buf + Send + 'async_trait,
        'l1: 'async_trait,
        Self: 'async_trait,
    {
        (**self).send_data(data, end_of_stream)
    }

    fn send_trailers<'l1, 'async_trait>(
        &'l1 mut self,
        trailers: HeaderMap,
    ) -> Pin<Box<dyn Future<Output = Result<(), Self::Error>> + Send + 'async_trait>>
    where
        'l1: 'async_trait,
        Self: 'async_trait,
    {
        (**self).send_trailers(trailers)
    }
}
