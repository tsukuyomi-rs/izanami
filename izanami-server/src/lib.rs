//! An HTTP server implementation powered by `hyper` and `tower-service`.

#![doc(html_root_url = "https://docs.rs/izanami-server/0.1.0-preview.2")]
#![deny(
    missing_debug_implementations,
    nonstandard_style,
    rust_2018_idioms,
    rust_2018_compatibility,
    unused
)]
#![forbid(clippy::unimplemented)]

mod drain;
mod server;
mod service;
mod util;

pub use crate::{
    server::{Builder, Incoming, Server},
    service::{
        HttpRequest, //
        HttpResponse,
        HttpService,
        IntoHttpService,
        MakeHttpService,
        OnUpgrade,
        RequestBody,
        ResponseBody,
        Upgraded,
    },
};

#[allow(dead_code)]
type BoxedStdError = Box<dyn std::error::Error + Send + Sync + 'static>;
