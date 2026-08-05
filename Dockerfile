# syntax=docker/dockerfile:1
FROM rust:1-trixie@sha256:3382bd20aa942806c533e9a73cd000474fb3ef173f71e684cc9b942675781769 AS builder
WORKDIR /app
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/app/target \
    cargo build --release && \
    cp target/release/nginx-lint /usr/local/bin/nginx-lint

FROM debian:trixie-slim@sha256:020c0d20b9880058cbe785a9db107156c3c75c2ac944a6aa7ab59f2add76a7bd
COPY --from=builder /usr/local/bin/nginx-lint /usr/local/bin/nginx-lint
ENTRYPOINT ["nginx-lint"]
