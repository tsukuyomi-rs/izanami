use async_trait::async_trait;
use http::{Request, Response};
use std::future::Future;
use std::io;
use std::pin::Pin;

#[async_trait]
pub trait Eventer: Send {
    type Error: Into<Box<dyn std::error::Error + Send + Sync>>;

    async fn send_response<T: AsRef<[u8]>>(
        &mut self,
        response: Response<T>,
    ) -> Result<(), Self::Error>
    where
        T: Send;
}

impl<E> Eventer for &mut E
where
    E: Eventer,
{
    type Error = E::Error;

    fn send_response<'l1, 'async_trait, T>(
        &'l1 mut self,
        response: Response<T>,
    ) -> Pin<Box<dyn Future<Output = Result<(), Self::Error>> + Send + 'async_trait>>
    where
        T: AsRef<[u8]> + Send + 'async_trait,
        'l1: 'async_trait,
        Self: 'async_trait,
    {
        (**self).send_response(response)
    }
}

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
