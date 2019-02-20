use {
    echo_service::Echo, //
    http::Response,
    izanami::no_tls,
};

#[cfg(unix)]
fn main() -> izanami::Result<()> {
    let echo = Echo::builder()
        .add_route("/", |_cx| {
            Response::builder() //
                .body("Hello")
                .unwrap()
        })?
        .build();

    izanami::run_unix("/tmp/echo-service.sock", no_tls(), echo)
}

#[cfg(not(unix))]
fn main() {
    println!("This example works only on Unix platform.")
}
