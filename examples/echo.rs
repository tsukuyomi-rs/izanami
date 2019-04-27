use {
    futures::{Async, Poll},
    http::Response,
    izanami::{context::Context, handler::Handler, launcher::Launcher},
};

#[derive(Default)]
struct Echo(());

impl Handler for Echo {
    type Body = String;
    type Error = std::convert::Infallible;

    fn poll_response(&mut self, _: &mut Context<'_>) -> Poll<Response<Self::Body>, Self::Error> {
        Ok(Async::Ready(
            Response::builder()
                .header("content-type", "text/plain")
                .body("Hello, world!\n".into())
                .unwrap(),
        ))
    }
}

fn main() -> failure::Fallible<()> {
    let mut launcher = Launcher::new(Echo::default)?;
    launcher.bind("127.0.0.1:4000")?;
    launcher.run_forever();
    Ok(())
}
