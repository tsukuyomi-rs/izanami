use {echo_service::Echo, http::Response};

fn main() -> izanami::Result<()> {
    let echo = Echo::builder()
        .add_route("/", |_cx| {
            Response::builder() //
                .body("Hello")
                .unwrap()
        })?
        .build();

    izanami::Server::build() //
        .serve(echo)
}
