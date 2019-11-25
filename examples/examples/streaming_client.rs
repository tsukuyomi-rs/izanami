use tokio::io::{stdin, AsyncReadExt};
use tokio::net::TcpStream;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let stream = TcpStream::connect("127.0.0.1:4000").await?;

    let (mut h2, conn) = h2::client::handshake(stream).await?;
    tokio::spawn(async move {
        if let Err(err) = conn.await {
            eprintln!("connection background error: {}", err);
        }
    });
    futures::future::poll_fn(|cx| h2.poll_ready(cx)).await?;

    let (response, mut sender) = h2.send_request(http::Request::new(()), false)?;

    let response = response.await?;
    anyhow::ensure!(
        response.status() == http::StatusCode::OK,
        "response is not OK"
    );

    let (_, mut receiver) = response.into_parts();
    tokio::spawn(async move {
        while let Some(data) = receiver.data().await {
            match data {
                Ok(data) => {
                    let release_capacity = receiver.release_capacity();
                    release_capacity.release_capacity(data.len()).unwrap();
                    println!("recv: {:?}", data);
                }
                Err(err) => {
                    eprintln!("receive error: {}", err);
                    return;
                }
            }
        }
    });

    let mut stdin = stdin();
    let mut buf = vec![0u8; 1024];
    loop {
        let count = stdin.read(&mut buf[..]).await?;
        let data = std::str::from_utf8(&buf[..count])?.trim();
        if data.is_empty() {
            continue;
        }

        println!("send {:?}", data);
        sender.reserve_capacity(data.len());
        let _cap = futures::future::poll_fn(|cx| sender.poll_capacity(cx))
            .await
            .transpose()?;
        sender.send_data(data.into(), false)?;
    }
}
