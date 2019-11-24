use crate::websocket::Message;
use async_trait::async_trait;
use bytes::Buf;
use http::{HeaderMap, Request, Response};
use std::{error::Error, future::Future, pin::Pin};

#[async_trait]
pub trait Events: Send {
    type Data: Buf + Send;
    type Error: Error + Send + Sync + 'static;
    type PushEvents: PushEvents<Error = Self::Error>;

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

    async fn push_request(&mut self, request: Request<()>)
        -> Result<Self::PushEvents, Self::Error>;

    async fn start_websocket(&mut self, response: Response<()>) -> Result<(), Self::Error>;
    async fn websocket_message(&mut self) -> Result<Option<Message>, Self::Error>;
    async fn send_websocket_message(&mut self, message: Message) -> Result<(), Self::Error>;
}

impl<E: ?Sized> Events for &mut E
where
    E: Events,
{
    type Data = E::Data;
    type Error = E::Error;
    type PushEvents = E::PushEvents;

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

    fn push_request<'l1, 'async_trait>(
        &'l1 mut self,
        request: Request<()>,
    ) -> crate::BoxFuture<'async_trait, Result<Self::PushEvents, Self::Error>>
    where
        'l1: 'async_trait,
        Self: 'async_trait,
    {
        (**self).push_request(request)
    }

    fn start_websocket<'l1, 'async_trait>(
        &'l1 mut self,
        response: Response<()>,
    ) -> crate::BoxFuture<'async_trait, Result<(), Self::Error>>
    where
        'l1: 'async_trait,
        Self: 'async_trait,
    {
        (**self).start_websocket(response)
    }

    fn websocket_message<'l1, 'async_trait>(
        &'l1 mut self,
    ) -> crate::BoxFuture<'async_trait, Result<Option<Message>, Self::Error>>
    where
        'l1: 'async_trait,
        Self: 'async_trait,
    {
        (**self).websocket_message()
    }

    fn send_websocket_message<'l1, 'async_trait>(
        &'l1 mut self,
        message: Message,
    ) -> crate::BoxFuture<'async_trait, Result<(), Self::Error>>
    where
        'l1: 'async_trait,
        Self: 'async_trait,
    {
        (**self).send_websocket_message(message)
    }
}

impl<E: ?Sized> Events for Box<E>
where
    E: Events,
{
    type Data = E::Data;
    type Error = E::Error;
    type PushEvents = E::PushEvents;

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

    fn push_request<'l1, 'async_trait>(
        &'l1 mut self,
        request: Request<()>,
    ) -> crate::BoxFuture<'async_trait, Result<Self::PushEvents, Self::Error>>
    where
        'l1: 'async_trait,
        Self: 'async_trait,
    {
        (**self).push_request(request)
    }

    fn start_websocket<'l1, 'async_trait>(
        &'l1 mut self,
        response: Response<()>,
    ) -> crate::BoxFuture<'async_trait, Result<(), Self::Error>>
    where
        'l1: 'async_trait,
        Self: 'async_trait,
    {
        (**self).start_websocket(response)
    }

    fn websocket_message<'l1, 'async_trait>(
        &'l1 mut self,
    ) -> crate::BoxFuture<'async_trait, Result<Option<Message>, Self::Error>>
    where
        'l1: 'async_trait,
        Self: 'async_trait,
    {
        (**self).websocket_message()
    }

    fn send_websocket_message<'l1, 'async_trait>(
        &'l1 mut self,
        message: Message,
    ) -> crate::BoxFuture<'async_trait, Result<(), Self::Error>>
    where
        'l1: 'async_trait,
        Self: 'async_trait,
    {
        (**self).send_websocket_message(message)
    }
}

#[async_trait]
pub trait PushEvents: Send {
    type Error: Error + Send + Sync + 'static;

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
