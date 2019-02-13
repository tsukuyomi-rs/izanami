use {
    echo_service::Echo, //
    http::Response,
    izanami::http::HttpServer,
};

fn main() -> izanami::Result<()> {
    let echo = Echo::builder()
        .add_route("/", |_cx| {
            Response::builder() //
                .body("Hello")
                .unwrap()
        })?
        .build();

    HttpServer::new(move || echo.clone()) //
        .bind("127.0.0.1:5000")?
        .bind("127.0.0.1:6000")?
        .run()
}
