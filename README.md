# `izanami`

> This library is in the experimental stage, and it cannot be used for production use.

An HTTP server implementation powered by `hyper` and `tower-service`.

## Features

* Supports Both of HTTP/1.x and HTTP/2.0 protocols with the power of `hyper`
* [Unix domain socket](./examples/uds-server) support (only on Unix platform)

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
