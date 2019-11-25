use http::{Request, Response};
use izanami_h2::{H2Events, H2Server};
use std::io;

async fn app(_: Request<()>, mut events: H2Events<'_>) -> anyhow::Result<()> {
    events.start_send_response(Response::new(())).await?;

    while let Some(data) = events.data().await? {
        eprintln!("recv: {:?}", data);
        events.send_data(data, false).await?
    }

    Ok(())
}

#[tokio::main]
async fn main() -> io::Result<()> {
    let server = H2Server::bind("127.0.0.1:4000").await?;
    server.serve(app).await?;

    Ok(())
}
