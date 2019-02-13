use {
    echo_service::Echo, //
    http::Response,
    izanami::http::HttpServer,
    std::path::Path,
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

    HttpServer::new(move || echo.clone()) //
        .bind(Path::new("/tmp/echo-service.sock"))?
        .run_local()
}

#[cfg(not(unix))]
fn main() {
    println!("This example works only on Unix platform.")
}
