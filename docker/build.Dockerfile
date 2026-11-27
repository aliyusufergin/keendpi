FROM rust:1-alpine
RUN rustup target add aarch64-unknown-linux-musl
WORKDIR /src
