use {
    echo_service::Echo, //
    http::Response,
    izanami::HttpServer,
    std::path::Path,
    tokio::runtime::Runtime,
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

    let mut rt = Runtime::new()?;
    HttpServer::new(move || echo.clone()) //
        .bind(Path::new("/tmp/echo-service.sock"))?
        .run(&mut rt)
}

#[cfg(not(unix))]
fn main() {
    println!("This example works only on Unix platform.")
}
