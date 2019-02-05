#!/bin/bash

CA_SUBJECT="/C=JP/ST=Tokyo/O=Izanami CA/CN=localhost"
SUBJECT="/C=JP/ST=Tokyo/O=Izanami/CN=localhost"

DIR="$(cd $(dirname $BASH_SOURCE); pwd)"
cd $DIR

set -ex

# generate RSA private key
openssl genrsa -out ca_key.pem 4096

# create Certificate Signing Request
openssl req -new -x509 \
  -days 3650 \
  -key ca_key.pem \
  -subj "${CA_SUBJECT}" \
  -out ca_cert.pem

openssl req -newkey rsa:4096 -nodes -sha256 \
  -keyout key.pem \
  -subj "${SUBJECT}" \
  -out server-csr.pem

openssl x509 -req -sha256 \
  -days 3650 \
  -CA ca_cert.pem \
  -CAkey ca_key.pem \
  -CAcreateserial \
  -in server-csr.pem \
  -out cert.pem
