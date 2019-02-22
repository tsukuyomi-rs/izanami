# `izanami`

[![crates.io](https://img.shields.io/crates/v/izanami.svg)](https://crates.io/crates/izanami)
[![rust toolchain](https://img.shields.io/badge/rust%20toolchain-1.31.1%2B-yellowgreen.svg)](https://blog.rust-lang.org/2018/12/20/Rust-1.31.1.html)
[![Build Status](https://ubnt-intrepid.visualstudio.com/izanami/_apis/build/status/ubnt-intrepid.izanami?branchName=master)](https://ubnt-intrepid.visualstudio.com/izanami/_build/latest?definitionId=2?branchName=master)
[![codecov](https://codecov.io/gh/ubnt-intrepid/izanami/branch/master/graph/badge.svg)](https://codecov.io/gh/ubnt-intrepid/izanami)

> This library is in the experimental stage, and it cannot be used for production use.

An HTTP server implementation powered by `hyper` and `tower-service`.

## Features

* Supports Both of HTTP/1.x and HTTP/2.0 protocols with the power of `hyper`
* [Unix domain socket](./examples/uds-server) support (only on Unix platform)
* Built-in SSL/TLS support ([`native-tls`](./examples/native-tls-server), [`openssl`](./examples/openssl-server)
 or [`rustls`](./examples/rustls-server) at your option)

Note that this project does **not** aim to grow itself to a Web framework.
The *goal* of Izanami is to provide a foundation for Web frameworks that can
be used in common (it just corresponds to the position of Werkzeug against Flask).

## Resources

* [API documentation (master)](https://ubnt-intrepid.github.io/izanami)
* [API documentation (released)](https://docs.rs/izanami)
* [Examples](./examples)

## License

This project is licensed under either of

* MIT license ([LICENSE-MIT](./LICENSE-MIT) or http://opensource.org/licenses/MIT)
* Apache License, Version 2.0 ([LICENSE-APACHE](./LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)

at your option.

<!-- links -->

[`hyper`]: https://github.com/hyperium/hyper
[`tokio`]: https://github.com/tokio-rs/tokio
