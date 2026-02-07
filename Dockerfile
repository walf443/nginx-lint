FROM rust:1-bookworm AS builder
RUN rustup target add wasm32-unknown-unknown
WORKDIR /app
COPY . .
RUN make -j$(nproc) build-plugins
RUN make build

FROM debian:bookworm-slim
COPY --from=builder /app/target/release/nginx-lint /usr/local/bin/nginx-lint
ENTRYPOINT ["nginx-lint"]
