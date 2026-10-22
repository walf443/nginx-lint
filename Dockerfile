# syntax=docker/dockerfile:1
FROM rust:1-trixie@sha256:931c2c0531a8b143e044f0f3914dfe802f15bb3d1955f0b657e9359327ac2ffc AS builder
WORKDIR /app
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/app/target \
    cargo build --release && \
    cp target/release/nginx-lint /usr/local/bin/nginx-lint

FROM debian:trixie-slim@sha256:62bd423aeee912639ca8ee681c2b762cb3e25137e0a92b20be455898a9f034ad
COPY --from=builder /usr/local/bin/nginx-lint /usr/local/bin/nginx-lint
ENTRYPOINT ["nginx-lint"]
