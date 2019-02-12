#!/bin/bash

set -ex

cd $(cargo metadata --format-version=1 | python3 -c 'import json,sys; print(json.load(sys.stdin)["workspace_root"])')
cd target/doc

git init
git remote add upstream "https://${GH_TOKEN}@github.com/ubnt-intrepid/izanami.git"
git config user.name 'Yusuke Sasaki'
git config user.email 'yusuke.sasaki.nuem@gmail.com'

git add -A .
git commit -qm "Build API doc at $(git rev-parse --short HEAD)"

git push -q upstream HEAD:refs/heads/gh-pages --force
