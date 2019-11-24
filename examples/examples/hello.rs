use async_trait::async_trait;
use http::{Request, Response};
use izanami_h2::{H2Events, H2Server};
use std::io;

struct App;

#[async_trait]
impl<'a> izanami::App<H2Events<'a>> for App {
    async fn call(&self, _: &Request<()>, mut events: H2Events<'a>) -> anyhow::Result<()>
    where
        H2Events<'a>: 'async_trait,
    {
        events
            .send_response(
                Response::builder() //
                    .body(io::Cursor::new("Hello, world!\n"))
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
