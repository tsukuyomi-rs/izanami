use {
    futures::Future,
    http::Response,
    izanami_server::{
        net::tcp::AddrIncoming, //
        protocol::H1,
        Server,
    },
    izanami_service::{service_fn, ServiceExt},
    std::io,
};

fn main() -> io::Result<()> {
    let h1 = H1::new();
    let server = Server::new(
        AddrIncoming::bind("127.0.0.1:5000")? //
            .service_map(move |stream| {
                let remote_addr = stream.remote_addr();
                h1.serve(
                    stream,
                    service_fn(move |_req| -> io::Result<_> {
                        let _ = &remote_addr;
                        Ok(Response::builder()
                            .header("content-type", "text/plain")
                            .body("Hello")
                            .expect("valid response"))
                    }),
                )
            }),
    )
    .map_err(|e| eprintln!("server error: {}", e));

    tokio::run(server);
    Ok(())
}
