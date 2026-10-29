# syntax=docker/dockerfile:1
FROM rust:1-trixie@sha256:09027f873d45b8ecd1dc86f1fa13b7a62b875c961f017a0e331267b350430137 AS builder
WORKDIR /app
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/app/target \
    cargo build --release && \
    cp target/release/nginx-lint /usr/local/bin/nginx-lint

FROM debian:trixie-slim@sha256:f6b08b09a05725f9431502d3b5714f5c4576e82c1a84fa1debe0c24a7e0c3ec4
COPY --from=builder /usr/local/bin/nginx-lint /usr/local/bin/nginx-lint
ENTRYPOINT ["nginx-lint"]
