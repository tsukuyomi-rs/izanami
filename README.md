<h1 align="center">
  <code>izanami</code>
</h1>
<div align="center">
  <strong>
    A simple Web application interface inspired from <a href="https://asgi.readthedocs.io/en/latest/index.html">ASGI</a>.
  </strong>
</div>

<br />

> This library is in the experimental stage, and it cannot be used for production use.

The goal of this project is to establish a Web application interface that focuses on simplicity and ease of extension, and to provide a reference implementation of HTTP server based on its interface.

## Rationale

Many of today's HTTP servers and Web frameworks for Rust use the Web application model that takes an HTTP request as an argument and returns the corresponding response as a return value.
Though this model works well for many Web applications with RESTful architecture, it has the following drawbacks:

* It is difficult to write the cleanup processes after the application returns the response as the application logic.
  In many cases, the application needs to wait for responding to the client until the cleanup processes are completed, or spawn a task to continue the cleanup on the background.

* In some use cases, such as WebSockets, the above model that ends the application logic with returning the response does not match.

* The application needs to prepare the concrete type that represengs the response body.
  If the application consists of multiple branch and they have
  different body types, it needs to provide an enum that summarizes their types.

This project aims to establish another Web application model that can overcome the above drawbacks and that can bridge existing Web applications using the traditional (RPC-like) applications.

## Status

WIP

## License

This project is licensed under either of

* MIT license ([LICENSE-MIT](./LICENSE-MIT) or http://opensource.org/licenses/MIT)
* Apache License, Version 2.0 ([LICENSE-APACHE](./LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)

at your option.
