use async_trait::async_trait;
use http::{HeaderValue, Request, Response};
use izanami_h2::{H2Events, H2Server};
use std::io;

struct App;

#[async_trait]
impl<'a> izanami::App<H2Events<'a>> for App {
    type Error = Box<dyn std::error::Error + Send + Sync>;

    async fn call(&self, _: Request<()>, mut events: H2Events<'a>) -> Result<(), Self::Error> {
        events
            .send_response(
                Response::builder() //
                    .header("content-length", HeaderValue::from_static("14"))
                    .body("Hello, world!\n")
                    .unwrap(),
            )
            .await?;

        Ok(())
    }
}

#[tokio::main]
async fn main() -> io::Result<()> {
    let server = H2Server::bind("127.0.0.1:4000").await?;
    server.serve(&App).await?;

    Ok(())
}
