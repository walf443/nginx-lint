# syntax=docker/dockerfile:1
FROM rust:1-trixie@sha256:4fd8406017c992f7b8ab55a2f99a1d56aeb1d7ecd255850dfa04239a88601f73 AS builder
WORKDIR /app
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/app/target \
    cargo build --release && \
    cp target/release/nginx-lint /usr/local/bin/nginx-lint

FROM debian:trixie-slim@sha256:545a1665d9364d3b00d1c892aa8fabc88d3c1f1d673eeeedfa3051010ebd91bb
COPY --from=builder /usr/local/bin/nginx-lint /usr/local/bin/nginx-lint
ENTRYPOINT ["nginx-lint"]
