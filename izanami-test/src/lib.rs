//! The basic utility for testing HTTP services.
//!
//! The purpose of this crate is to provide the way to test HTTP services
//! used by `izanami` server without using the low level I/O.
//!
//! # Example
//!
//! ```
//! # #![deny(deprecated)]
//! use {
//!     http::{Request, Response},
//!     izanami_service::MakeService,
//!     izanami_test::{Server, runtime::Awaitable},
//! };
//! # use {
//! #   izanami_service::Service,
//! #   std::io,
//! # };
//!
//! # fn test_echo() -> izanami_test::Result<()> {
//! # izanami_test::runtime::with_default(|rt| {
//! // the target service factory to be tested.
//! let make_service = {
//!     struct Echo(());
//!
//!     impl<Ctx, Bd> MakeService<Ctx, Request<Bd>> for Echo {
//!         // ...
//! #       type Response = Response<String>;
//! #       type Error = io::Error;
//! #       type Service = Echo;
//! #       type MakeError = io::Error;
//! #       type Future = futures::future::FutureResult<Self::Service, Self::MakeError>;
//! #       fn make_service(&self, _: Ctx) -> Self::Future { futures::future::ok(Echo(())) }
//!     }
//! #   impl<Bd> Service<Request<Bd>> for Echo {
//! #       type Response = Response<String>;
//! #       type Error = io::Error;
//! #       type Future = futures::future::FutureResult<Self::Response, Self::Error>;
//! #       fn poll_ready(&mut self) -> futures::Poll<(), Self::Error> { Ok(().into()) }
//! #       fn call(&mut self, _: Request<Bd>) -> Self::Future { futures::future::ok(Response::new("hello".to_string())) }
//! #   }
//!
//!     Echo(())
//! };
//!
//! // create a `Server`, that contains the specified
//! // service factory and a runtime for driving the inner
//! // asynchronous tasks.
//! let mut server = Server::new(make_service);
//!
//! // create a `Client` to test an established connection
//! // with the peer.
//! let mut client = server.client().wait(rt)?;
//!
//! // applies an HTTP request to the client and await
//! // its response.
//! //
//! // the method simulates the behavior of service until
//! // just before starting to send the response body.
//! let response = client
//!     .respond(
//!         Request::get("/").body(())?
//!     )
//!     .wait(rt)?;
//! assert_eq!(response.status(), 200);
//!
//! // drive the response body and await its result.
//! let body = response.send_body().wait(rt)?;
//! assert_eq!(body.to_utf8()?, "hello");
//! # Ok(())
//! # })
//! # }
//! # fn main() {}
//! ```

#![doc(html_root_url = "https://docs.rs/izanami-test/0.1.0-preview.1")]
#![deny(
    missing_debug_implementations,
    nonstandard_style,
    rust_2018_idioms,
    rust_2018_compatibility,
    unused
)]
#![forbid(clippy::unimplemented)]

pub mod client;
mod error;
pub mod runtime;
mod server;
pub mod service;

pub use crate::{
    error::{Error, Result},
    server::Server,
};
