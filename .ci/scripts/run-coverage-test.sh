#!/bin/bash

set -ex

cargo tarpaulin --all --out Xml
bash <(curl -s https://codecov.io/bash)

cargo tarpaulin --packages izanami --all-features --out Xml
bash <(curl -s https://codecov.io/bash)
