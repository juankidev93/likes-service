FROM rust:1-bookworm AS builder

WORKDIR /app

COPY Cargo.toml Cargo.lock ./
COPY src ./src

RUN cargo build --release --locked

FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && groupadd --system likes \
    && useradd --system --gid likes --create-home --home-dir /home/likes --shell /usr/sbin/nologin likes \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/likes-service /usr/local/bin/likes-service

ENV SERVICE_HOST=0.0.0.0
ENV HTTP_PORT=3000

EXPOSE 3000

USER likes

HEALTHCHECK --interval=10s --timeout=3s --start-period=10s --retries=5 \
  CMD curl -fsS "http://127.0.0.1:${HTTP_PORT}/health/live" || exit 1

CMD ["likes-service"]
