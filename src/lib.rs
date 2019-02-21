//! An HTTP server implementation powered by `hyper` and `tower-service`.

#![doc(html_root_url = "https://docs.rs/izanami/0.1.0-preview.2")]
#![deny(
    missing_debug_implementations,
    nonstandard_style,
    rust_2018_idioms,
    rust_2018_compatibility,
    unused
)]
#![forbid(clippy::unimplemented)]

#[doc(no_inline)]
pub use {
    izanami_buf as buf, //
    izanami_http as http,
    izanami_net as net,
    izanami_rt as rt,
    izanami_server as server,
    izanami_service as service,
    izanami_util as util,
};
