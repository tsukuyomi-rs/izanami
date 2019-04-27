#[cfg(unix)]
mod imp {
    use {
        futures::Future,
        http::Response,
        izanami_server::{
            net::unix::AddrIncoming,
            protocol::H1,
            Server, //
        },
        izanami_service::{service_fn, ServiceExt},
        std::io,
    };

    pub fn main() -> io::Result<()> {
        let protocol = H1::new();
        let server = Server::new(
            AddrIncoming::bind("/tmp/echo-service.sock")? //
                .service_map(move |stream| {
                    protocol.serve(
                        stream,
                        service_fn(|_req| {
                            Response::builder()
                                .header("content-type", "text/plain")
                                .body("Hello")
                        }),
                    )
                }),
        )
        .map_err(|_| unimplemented!());

        tokio::run(server);
        Ok(())
    }
}

#[cfg(not(unix))]
mod imp {
    pub fn main() -> std::io::Result<()> {
        println!("This example works only on Unix platform.");
        Ok(())
    }
}

fn main() -> std::io::Result<()> {
    crate::imp::main()
}
