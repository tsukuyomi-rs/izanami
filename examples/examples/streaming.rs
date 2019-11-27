use async_trait::async_trait;
use http::{Request, Response};
use izanami_h2::{Events, Server};

#[derive(Clone)]
struct Streaming;

#[async_trait]
impl<'a> izanami::App<Events<'a>> for Streaming {
    type Error = anyhow::Error;

    async fn call(&self, _: Request<()>, mut events: Events<'a>) -> Result<(), Self::Error> {
        events.start_send_response(Response::new(()), false).await?;

        while let Some(data) = events.data().await.transpose()? {
            eprintln!("recv: {:?}", data);
            events.send_data(data, false).await?
        }

        Ok(())
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let server = Server::bind("127.0.0.1:4000").await?;
    server.serve(Streaming).await?;

    Ok(())
}
