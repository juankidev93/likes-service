FROM rust:1-bookworm AS builder

WORKDIR /app

COPY Cargo.toml Cargo.lock ./
COPY src ./src

RUN cargo build --release --locked

FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/likes-service /usr/local/bin/likes-service

ENV SERVICE_HOST=0.0.0.0
ENV SERVICE_PORT=3000

EXPOSE 3000

CMD ["likes-service"]
