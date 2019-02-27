#!/bin/bash

CA_SUBJECT="/C=JP/ST=Tokyo/O=Izanami CA/CN=localhost"

DIR="$(cd $(dirname $BASH_SOURCE); pwd)"
cd $DIR

set -ex

# generate RSA private key
openssl genrsa -out server-key.pem 4096

# create Certificate Signing Request
openssl req -new \
  -subj "${CA_SUBJECT}" \
  -key server-key.pem \
  -out server-csr.pem

# create server certificate
openssl x509 -req \
  -days 3650 \
  -signkey server-key.pem \
  -in server-csr.pem \
  -out server-crt.pem

# export certificate as PKCS12 format
openssl pkcs12 -export \
  -name "izanami" \
  -password "pass:mypass" \
  -inkey server-key.pem \
  -in server-crt.pem \
  -out identity.pfx
