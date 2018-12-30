FROM rust:latest

ENV SCCACHE_VERSION 0.2.7
RUN wget https://github.com/mozilla/sccache/releases/download/${SCCACHE_VERSION}/sccache-${SCCACHE_VERSION}-x86_64-unknown-linux-musl.tar.gz \
    && tar -xzvf sccache-${SCCACHE_VERSION}-x86_64-unknown-linux-musl.tar.gz \
    && mv sccache-${SCCACHE_VERSION}-x86_64-unknown-linux-musl/sccache /usr/local/bin/ \
    && rm -rf sccache-${SCCACHE_VERSION}-x86_64-unknown-linux-musl*

ENV RUSTC_WRAPPER=sccache
