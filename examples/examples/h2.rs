use http::{HeaderValue, Request, Response};
use izanami_h2::{Events, Server};

async fn app(_: Request<()>, mut events: Events<'_>) -> anyhow::Result<()> {
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

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let server = Server::bind("127.0.0.1:4000").await?;
    server.serve(app).await?;

    Ok(())
}
