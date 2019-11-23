use async_trait::async_trait;
use http::{Request, Response};
use izanami::Eventer;
use std::io;

struct App;

#[async_trait]
impl izanami::App for App {
    async fn call<E>(&self, _: &Request<()>, mut ev: E) -> io::Result<()>
    where
        E: Eventer,
    {
        ev.send_response(
            Response::builder() //
                .body(io::Cursor::new("Hello, world!\n"))
                .unwrap(),
        )
        .await
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

        Ok(())
    }
}

#[tokio::main]
async fn main() -> io::Result<()> {
    let server = izanami::h2::Server::bind("127.0.0.1:4000").await?;
    server.serve(&App).await?;

    Ok(())
}
