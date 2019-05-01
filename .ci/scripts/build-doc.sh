#!/bin/bash

set -ex

cargo doc --no-deps -p failure
cargo doc --no-deps -p tower-service
cargo doc --no-deps --all
rm -f target/doc/.lock

echo '<meta http-equiv="refresh" content="0;URL=izanami/index.html">' > target/doc/index.html
