# `izanami`

[![crates.io](https://img.shields.io/crates/v/izanami.svg)](https://crates.io/crates/izanami)
[![Build Status](https://travis-ci.org/ubnt-intrepid/izanami.svg?branch=master)](https://travis-ci.org/ubnt-intrepid/izanami)

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

## License

This project is licensed under either of

* MIT license ([LICENSE-MIT](./LICENSE-MIT) or http://opensource.org/licenses/MIT)
* Apache License, Version 2.0 ([LICENSE-APACHE](./LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)

at your option.

<!-- links -->

[`hyper`]: https://github.com/hyperium/hyper
[`tokio`]: https://github.com/tokio-rs/tokio
