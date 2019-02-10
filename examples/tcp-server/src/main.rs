use {
    echo_service::Echo, //
    http::Response,
    izanami::{Http, Server},
};

fn main() -> izanami::Result<()> {
    let echo = Echo::builder()
        .add_route("/", |_cx| {
            Response::builder() //
                .body("Hello")
                .unwrap()
        })?
        .build();

    let mut server = Server::default()?;
    server.start(
        Http::bind("127.0.0.1:5000") //
            .serve(move || echo.clone()),
    )?;
    server.run()
}
