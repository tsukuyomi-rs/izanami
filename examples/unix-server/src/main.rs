use {
    echo_service::Echo, //
    http::Response,
    izanami::no_tls,
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

    izanami::run_unix("/tmp/echo-service.sock", no_tls(), echo)
        .expect("failed to start the server");
}

#[cfg(not(unix))]
fn main() {
    println!("This example works only on Unix platform.")
}
