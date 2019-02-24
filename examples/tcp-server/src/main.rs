use {
    http::Response,
    izanami::{
        server::{Incoming, Server}, //
        service::service_fn_ok,
    },
    std::io,
};

fn main() -> io::Result<()> {
    let service = service_fn_ok(|()| {
        service_fn_ok(|_req| {
            Response::builder()
                .header("content-type", "text/plain")
                .body("Hello")
                .unwrap()
        })
    });

    let incoming_service = Incoming::bind_tcp("127.0.0.1:5000")? //
        .serve(service);

    let server = Server::new(incoming_service);
    izanami::rt::run(server);

    Ok(())
}
