use {
    echo_service::Echo,
    http::{Request, Response},
    izanami::runtime::Block,
    izanami_service::{MakeService, Service},
    tokio::runtime::current_thread::Runtime,
};

#[test]
fn test_empty_routes() -> izanami::Result<()> {
    let mut rt = Runtime::new()?;

    let mut echo = Echo::builder().build();

    let mut service = echo
        .make_service(()) //
        .block(&mut rt)?;

    let response = service
        .call(Request::get("/").body(())?) //
        .block(&mut rt)?;
    assert_eq!(response.status(), 404);

    Ok(())
}

#[test]
fn test_single_route() -> izanami::Result<()> {
    let mut rt = Runtime::new()?;

    let mut echo = Echo::builder() //
        .add_route("/", |_| {
            Response::builder() //
                .body("hello")
                .unwrap()
        })?
        .build();

    let mut service = echo
        .make_service(()) //
        .block(&mut rt)?;

    let response = service
        .call(
            Request::get("/") //
                .body(())?,
        )
        .block(&mut rt)?;
    assert_eq!(response.status(), 200);
    assert_eq!(std::str::from_utf8(&*response.body())?, "hello");

    Ok(())
}

#[test]
fn test_capture_param() -> izanami::Result<()> {
    let mut rt = Runtime::new()?;

    let mut echo = Echo::builder() //
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
        .build();

    let mut service = echo
        .make_service(()) //
        .block(&mut rt)?;

    let response = service
        .call(Request::get("/42").body(())?) //
        .block(&mut rt)?;
    assert_eq!(response.status(), 200);
    assert_eq!(std::str::from_utf8(&*response.body())?, "id=42");

    let response = service
        .call(Request::get("/fox").body(())?)
        .block(&mut rt)?;
    assert_eq!(response.status(), 404);

    Ok(())
}
