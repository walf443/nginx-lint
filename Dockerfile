FROM rust:1-trixie AS builder
RUN rustup target add wasm32-unknown-unknown
RUN cargo install wasm-tools
WORKDIR /app
COPY . .
RUN make -j"$(nproc)" build-plugins
RUN make build

FROM debian:trixie-slim
COPY --from=builder /app/target/release/nginx-lint /usr/local/bin/nginx-lint
ENTRYPOINT ["nginx-lint"]
