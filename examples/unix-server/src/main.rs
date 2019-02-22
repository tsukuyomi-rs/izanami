use {
    echo_service::Echo, //
    http::Response,
    izanami::{net::tls::no_tls, server::Server},
};

#[cfg(unix)]
fn main() {
    let echo = Echo::builder()
        .add_route("/", |_cx| {
            Response::builder() //
                .body("Hello")
                .unwrap()
        })
        .expect("invalid route")
        .build();

    izanami::rt::run(
        Server::bind_unix("/tmp/echo-service.sock", no_tls()) //
            .unwrap()
            .serve(echo),
    )
}

#[cfg(not(unix))]
fn main() {
    println!("This example works only on Unix platform.")
}
