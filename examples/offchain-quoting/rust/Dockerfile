# syntax=docker/dockerfile:1

FROM rust:1-bookworm AS builder
WORKDIR /app

COPY . .
RUN cargo build --release -p offchain-quoting-example-rust

FROM debian:bookworm-slim AS runtime

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --create-home appuser \
    && mkdir -p /app/examples/offchain-quoting/rust \
    && chown -R appuser /app

COPY --from=builder /app/target/release/offchain-quoting-example-rust /usr/local/bin/offchain-quoting-example-rust

WORKDIR /app/examples/offchain-quoting/rust
USER appuser
ENV RUST_LOG=info,offchain_quoting_example_rust=debug
ENTRYPOINT ["offchain-quoting-example-rust"]
