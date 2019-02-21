#!/bin/bash

set -ex

cargo doc --no-deps -p rustls
cargo doc --no-deps -p tokio-openssl
cargo doc --no-deps -p tokio-rustls
cargo doc --no-deps -p tower-service

cargo doc --no-deps -p izanami-buf
cargo doc --no-deps -p izanami-http
cargo doc --no-deps -p izanami-net --all-features
cargo doc --no-deps -p izanami-rt
cargo doc --no-deps -p izanami-server
cargo doc --no-deps -p izanami-service
cargo doc --no-deps -p izanami-util
cargo doc --no-deps -p izanami --all-features
cargo doc --no-deps -p echo-service
rm -f target/doc/.lock

echo '<meta http-equiv="refresh" content="0;URL=izanami/index.html">' > target/doc/index.html
