use async_trait::async_trait;
use http::{Request, Response};

#[derive(Clone, Default)]
pub struct Hello(());

#[async_trait]
impl<E> izanami::App<E> for Hello
where
    E: izanami::Events + Send,
    E::Data: Send,
    &'static str: Into<E::Data>,
{
    type Error = E::Error;

    async fn call(&self, request: Request<E>) -> Result<(), Self::Error>
    where
        E: 'async_trait,
    {
        let mut events = request.into_body();

        events
            .start_send_response(
                Response::builder() //
                    .header("content-length", "14")
                    .body(())
                    .unwrap(),
                false,
            )
            .await?;

        events.send_data("Hello, world!\n".into(), true).await?;

        Ok(())
    }
}
