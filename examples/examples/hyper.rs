use async_trait::async_trait;
use http::{Request, Response};
use izanami_hyper::{Events, Server};

#[derive(Clone)]
struct Hello;

#[async_trait]
impl<'a> izanami::App<Events<'a>> for Hello {
    type Error = anyhow::Error;

    async fn call(&self, _: Request<()>, mut events: Events<'a>) -> Result<(), Self::Error> {
        events
            .send_response(
                Response::builder() //
                    .header("content-length", "14")
                    .body("Hello, world!\n")
                    .unwrap(),
            )
            .await?;

        Ok(())
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let server = Server::bind("127.0.0.1:4000").await?;
    server.serve(Hello).await?;

    Ok(())
}
