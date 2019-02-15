//! The basic utility for testing HTTP services.
//!
//! The purpose of this module is to provide the way to test HTTP services
//! without using the network I/O.
//!
//! # Example
//!
//! ```
//! # #![deny(deprecated)]
//! use {
//!     http::{Request, Response},
//!     izanami_service::Service,
//!     izanami::test::Server,
//! };
//! # use {
//! #   std::io,
//! # };
//!
//! # fn test_echo() -> izanami::Result<()> {
//! # izanami::system::current_thread(|sys| {
//! // the target service factory to be tested.
//! let make_service = {
//!     struct Echo(());
//!
//!     impl<Ctx> Service<Ctx> for Echo {
//!         // ...
//! #       type Response = EchoService;
//! #       type Error = io::Error;
//! #       type Future = futures::future::FutureResult<Self::Response, Self::Error>;
//! #       fn poll_ready(&mut self) -> futures::Poll<(), Self::Error> { Ok(().into()) }
//! #       fn call(&mut self, _: Ctx) -> Self::Future { futures::future::ok(EchoService(())) }
//!     }
//! #   struct EchoService(());
//! #   impl<Bd> Service<Request<Bd>> for EchoService {
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
//! let mut client = sys.block_on(server.client())?;
//!
//! // applies an HTTP request to the client and await
//! // its response.
//! //
//! // the method simulates the behavior of service until
//! // just before starting to send the response body.
//! let response = sys.block_on(
//!     client
//!         .respond(
//!             Request::get("/").body(())?
//!         )
//! )?;
//! assert_eq!(response.status(), 200);
//!
//! // drive the response body and await its result.
//! let body = sys.block_on(response.send_body())?;
//! assert_eq!(body.to_utf8()?, "hello");
//! # Ok(())
//! # })
//! # }
//! # fn main() {}
//! ```

pub mod client;
mod server;
pub mod service;

pub use self::server::Server;
