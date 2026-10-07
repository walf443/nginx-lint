# syntax=docker/dockerfile:1
FROM rust:1-trixie@sha256:6ff07edce8775d0f64be7aba9197229407301bddf2054d62c27b541a6238a181 AS builder
WORKDIR /app
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/app/target \
    cargo build --release && \
    cp target/release/nginx-lint /usr/local/bin/nginx-lint

FROM debian:trixie-slim@sha256:a29215f6a35e51e22adffa17f89e9d2ef06214e64a2bad10d765c46aea49f11f
COPY --from=builder /usr/local/bin/nginx-lint /usr/local/bin/nginx-lint
ENTRYPOINT ["nginx-lint"]
