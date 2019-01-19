#!/bin/bash

set -ex

cargo tarpaulin --all
bash <(https://codecov.io/bash)

cargo tarpaulin --package izanami --all-features
bash <(https://codecov.io/bash)
