#!/bin/bash

set -e

rm -f .codecov
curl -s https://codecov.io/bash -O .codecov

cargo tarpaulin --all
bash .codecov

cargo tarpaulin --package izanami --all-features
bash .codecov
