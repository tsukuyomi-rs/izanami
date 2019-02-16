use {
    echo_service::Echo,
    http::{Request, Response},
    izanami::{runtime::Block, test::Server},
    tokio::runtime::current_thread::Runtime,
};

#[test]
fn test_empty_routes() -> izanami::Result<()> {
    let mut rt = Runtime::new()?;

    let mut server = Server::new(
        Echo::builder() //
            .build(),
    );

    let mut client = server
        .client() //
        .block(&mut rt)?;

    let response = client
        .request(
            Request::get("/") //
                .body(())?,
        )
        .block(&mut rt)?;
    assert_eq!(response.status(), 404);

    Ok(())
}

#[test]
fn test_single_route() -> izanami::Result<()> {
    let mut rt = Runtime::new()?;

    let mut server = Server::new(
        Echo::builder() //
            .add_route("/", |_| {
                Response::builder() //
                    .body("hello")
                    .unwrap()
            })?
            .build(),
    );

    let mut client = server
        .client() //
        .block(&mut rt)?;

    let response = client
        .request(
            Request::get("/") //
                .body(())?,
        )
        .block(&mut rt)?;
    assert_eq!(response.status(), 200);

    let body = response
        .send_body() //
        .block(&mut rt)?;
    assert_eq!(body.to_utf8()?, "hello");

    Ok(())
}

#[test]
fn test_capture_param() -> izanami::Result<()> {
    let mut rt = Runtime::new()?;

    let mut server = Server::new(
        Echo::builder() //
            .add_route("/([0-9]+)", |cx| {
                match cx
                    .captures()
                    .and_then(|c| c.get(1))
                    .and_then(|m| m.as_str().parse::<u32>().ok())
                {
                    Some(id) => Response::builder()
                        .status(200)
                        .body(format!("id={}", id))
                        .unwrap(),
                    None => Response::builder()
                        .status(400)
                        .body("missing or invalid id".into())
                        .unwrap(),
                }
            })?
            .build(),
    );

    let mut client = server
        .client() //
        .block(&mut rt)?;

    let response = client
        .request(
            Request::get("/42") //
                .body(())?,
        )
        .block(&mut rt)?;
    assert_eq!(response.status(), 200);

    let body = response
        .send_body() //
        .block(&mut rt)?;
    assert_eq!(body.to_utf8()?, "id=42");

    let response = client
        .request(
            Request::get("/fox") //
                .body(())?,
        )
        .block(&mut rt)?;
    assert_eq!(response.status(), 404);

    Ok(())
}
