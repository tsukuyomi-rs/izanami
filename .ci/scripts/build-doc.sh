#!/bin/bash

set -ex

cargo doc --no-deps -p izanami-service
cargo doc --no-deps -p izanami-test
cargo doc --no-deps -p izanami-util
cargo doc --no-deps -p izanami --all-features
cargo doc --no-deps -p echo-service
rm -f target/doc/.lock

echo '<meta http-equiv="refresh" content="0;URL=izanami/index.html">' > target/doc/index.html
