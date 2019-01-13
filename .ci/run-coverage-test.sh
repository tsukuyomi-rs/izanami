#!/bin/bash

DIR="$(cd $(dirname $BASH_SOURCE); pwd)"

set -ex

kcov --version

$DIR/cargo-kcov.py --all
$DIR/cargo-kcov.py -p izanami --all-features

bash <(curl -s https://codecov.io/bash) -K -s "$DIR/../target/cov"
