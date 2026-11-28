# syntax=docker/dockerfile:1
FROM rust:1-trixie@sha256:e84068000361bf3d4a5ba2b7647f62893528b1eb9f847d7a23b493f5ce8ea254 AS builder
WORKDIR /app
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/app/target \
    cargo build --release && \
    cp target/release/nginx-lint /usr/local/bin/nginx-lint

FROM debian:trixie-slim@sha256:8aa5a7bf5a1955bddbe7d74fcdd0b91ab30e74362a8b73af672b0481b35b44a0
COPY --from=builder /usr/local/bin/nginx-lint /usr/local/bin/nginx-lint
ENTRYPOINT ["nginx-lint"]
