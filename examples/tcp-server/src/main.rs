use {
    echo_service::Echo, //
    http::Response,
    izanami::HttpServer,
};

fn main() -> izanami::Result<()> {
    izanami::system::default(|sys| {
        let echo = Echo::builder()
            .add_route("/", |_cx| {
                Response::builder() //
                    .body("Hello")
                    .unwrap()
            })?
            .build();

        HttpServer::new(move || echo.clone()) //
            .bind(vec!["127.0.0.1:5000", "127.0.0.1:6000"])?
            .run(sys)
    })
}
