# `izanami`

[![crates.io](https://img.shields.io/crates/v/izanami.svg)](https://crates.io/crates/izanami)
[![rust toolchain](https://img.shields.io/badge/rust%20toolchain-1.31.1%2B-yellowgreen.svg)](https://blog.rust-lang.org/2018/12/20/Rust-1.31.1.html)

> This library is in the experimental stage, and it cannot be used for production use.

A *meta* library for creating Web frameworks.

The ultimate goal of this library is to provide an HTTP server
that Web frameworks can use in common and a utility for testing
the Web services.

## Features

* Asynchronous HTTP server powered by [`tokio`] and [`hyper`]
  * Supports both of [TCP](./examples/tcp-server) and [Unix domain socket](./examples/uds-server)
  * TLS support
    - [`native-tls`](./examples/native-tls-server)
    - [`openssl`](./examples/openssl-server)
    - [`rustls`](./examples/rustls-server)
* Utility for testing HTTP services

## Status

* CI status - [![Build Status](https://ubnt-intrepid.visualstudio.com/izanami/_apis/build/status/ubnt-intrepid.izanami?branchName=master)](https://ubnt-intrepid.visualstudio.com/izanami/_build/latest?definitionId=2?branchName=master)
* Coverage - [![codecov](https://codecov.io/gh/ubnt-intrepid/izanami/branch/master/graph/badge.svg)](https://codecov.io/gh/ubnt-intrepid/izanami)
* Dependencies - [![dependency status](https://deps.rs/repo/github/ubnt-intrepid/izanami/status.svg)](https://deps.rs/repo/github/ubnt-intrepid/izanami)

## License

This project is licensed under either of

* MIT license ([LICENSE-MIT](./LICENSE-MIT) or http://opensource.org/licenses/MIT)
* Apache License, Version 2.0 ([LICENSE-APACHE](./LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)

at your option.

<!-- links -->

[`hyper`]: https://github.com/hyperium/hyper
[`tokio`]: https://github.com/tokio-rs/tokio
