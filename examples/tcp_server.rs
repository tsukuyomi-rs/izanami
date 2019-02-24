use {
    http::Response,
    izanami::{
        server::{Incoming, Server}, //
        service::{service_fn_ok, ServiceExt},
    },
    std::io,
};

fn main() -> io::Result<()> {
    let incoming_service = Incoming::bind_tcp("127.0.0.1:5000")? //
        .serve(izanami::service::unit())
        .fixed_service()
        .map(|((), stream, protocol)| {
            let remote_addr = stream.remote_addr();
            let service = service_fn_ok(move |_req| {
                eprintln!("remote_addr = {}", remote_addr);
                Response::builder()
                    .header("content-type", "text/plain")
                    .body("Hello")
                    .unwrap()
            });

            (service, stream, protocol)
        });

    let server = Server::new(incoming_service);
    izanami::rt::run(server);

    Ok(())
}
