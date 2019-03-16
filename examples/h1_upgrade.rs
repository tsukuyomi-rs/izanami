use {
    futures::Future,
    http::{Response, StatusCode},
    izanami::{
        h1::{H1Connection, H1Request},
        http::upgrade::MaybeUpgrade,
        net::tcp::AddrIncoming,
        server::Server, //
        service::{ext::ServiceExt, service_fn},
    },
    std::io,
};

fn main() -> io::Result<()> {
    let server = Server::new(
        AddrIncoming::bind("127.0.0.1:5000")? //
            .service_map(|stream| {
                H1Connection::build(stream) //
                    .finish(service_fn(move |req: H1Request| -> io::Result<_> {
                        let err_msg = match req.headers().get(http::header::UPGRADE) {
                            Some(h) if h == "foobar" => None,
                            Some(..) => {
                                Some("the header field `upgrade' must be equal to 'foobar'.")
                            }
                            None => Some("missing header field: `upgrade'"),
                        };

                        if let Some(err_msg) = err_msg {
                            let mut res = Response::new(MaybeUpgrade::Data(err_msg));
                            *res.status_mut() = StatusCode::BAD_REQUEST;
                            return Ok(res);
                        }

                        //
                        let foobar = izanami::http::upgrade::upgrade_fn(|stream| {
                            Ok(tokio::io::read_exact(stream, vec![0; 7])
                                .and_then(|(stream, buf)| {
                                    println!(
                                        "server[foobar] recv: {:?}",
                                        std::str::from_utf8(&buf)
                                    );
                                    tokio::io::write_all(stream, b"bar=foo")
                                })
                                .and_then(|(stream, _)| {
                                    println!("server[foobar] sent");
                                    tokio::io::shutdown(stream)
                                })
                                .and_then(|_| {
                                    println!("server[foobar] closed");
                                    Ok(())
                                }))
                        });

                        // When the response has a status code `101 Switching Protocols`, the connection
                        // upgrades the protocol using the associated response body.
                        let response = Response::builder()
                            .status(101)
                            .header("upgrade", "foobar")
                            .body(MaybeUpgrade::Upgrade(foobar))
                            .unwrap();

                        Ok(response)
                    }))
            }),
    )
    .map_err(|e| eprintln!("server error: {}", e));

    izanami::rt::run(server);
    Ok(())
}
