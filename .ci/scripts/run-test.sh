#!/bin/bash

set -ex

if cargo fmt --version >/dev/null 2>&1; then
    cargo fmt -- --check
fi

if cargo clippy --version >/dev/null 2>&1; then
    cargo clippy --all-features --all-targets
fi

cargo test
cargo test --no-default-features
cargo test --all-features
