#!/bin/bash

set -ex

if cargo fmt --version >/dev/null 2>&1; then
    cargo fmt -- --check
fi

if cargo clippy --version >/dev/null 2>&1; then
    cargo clippy --all --all-targets
    cargo clippy -p izanami --all-features --all-targets
fi

cargo test --all
cargo test -p izanami --no-default-features
cargo test -p izanami --all-features
