#![cfg(unix)]

use {
    crate::server::{
        net::unix::AddrIncoming,
        protocol::H1, //
        service::{service_fn, ServiceExt},
        Server,
    },
    futures::{Future, Stream},
    http::{Request, Response},
    hyper::{
        client::{
            connect::{Connect, Connected, Destination},
            Client,
        },
        Body,
    },
    std::{io, path::PathBuf},
    tempfile::Builder,
    tokio::{
        net::UnixStream, //
        runtime::current_thread::Runtime,
        sync::oneshot,
    },
};

#[test]
fn smoke() -> failure::Fallible<()> {
    let mut rt = Runtime::new()?;

    let sock_tempdir = Builder::new().prefix("izanami-tests").tempdir()?;
    let sock_path = sock_tempdir.path().join("connect.sock");

    let (tx_shutdown, rx_shutdown) = oneshot::channel();
    let server = Server::builder(
        AddrIncoming::bind(&sock_path)? //
            .service_map(|stream| {
                H1::new().serve(
                    stream,
                    service_fn(|_req| {
                        Response::builder()
                            .header("content-type", "text/plain")
                            .body("hello")
                    }),
                )
            }),
    )
    .with_graceful_shutdown(rx_shutdown)
    .build()
    .map_err(|e| eprintln!("server error: {}", e));
    rt.spawn(Box::new(server));

    let client = Client::builder() //
        .build(TestConnect {
            sock_path: sock_path.clone(),
        });

    let response = rt.block_on(
        client.request(
            Request::get("http://localhost/") //
                .body(Body::empty())?,
        ),
    )?;
    assert_eq!(response.status(), 200);

    let body = rt.block_on(response.into_body().concat2())?;
    assert_eq!(body.into_bytes(), "hello");

    drop(client);
    let _ = tx_shutdown.send(());
    rt.run().unwrap();
    Ok(())
}

struct TestConnect {
    sock_path: PathBuf,
}

impl Connect for TestConnect {
    type Transport = UnixStream;
    type Error = io::Error;
    type Future =
        Box<dyn Future<Item = (Self::Transport, Connected), Error = Self::Error> + Send + 'static>;

    fn connect(&self, _: Destination) -> Self::Future {
        Box::new(
            UnixStream::connect(&self.sock_path) //
                .map(|stream| (stream, Connected::new())),
        )
    }
}
