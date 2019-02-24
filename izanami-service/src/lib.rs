//! Abstraction of network services based on [`tower-service`].
//!
//! [`tower-service`]: https://crates.io/crates/tower-service

#![doc(html_root_url = "https://docs.rs/izanami-service/0.1.0-preview.1")]
#![deny(
    missing_docs,
    missing_debug_implementations,
    nonstandard_style,
    rust_2018_idioms,
    rust_2018_compatibility,
    unused
)]
#![forbid(clippy::unimplemented)]

mod into_service;
mod make_service;
mod service_fn;
mod service_mut;
mod service_ref;

#[doc(no_inline)]
pub use tower_service::Service;

pub use crate::{
    into_service::IntoService, //
    make_service::MakeService,
    service_fn::{service_fn, service_fn_ok},
    service_mut::ServiceMut,
    service_ref::ServiceRef,
};
