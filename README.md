# Likes Service

Rust HTTP microservice for managing likes on content items (`post`, `bonus_hunter`, `top_picks`).

Includes:
- like, unlike, count, status, batch, and user likes endpoints
- `GET /v1/likes/top` and `GET /v1/likes/stream`
- Postgres as the source of truth
- Redis for caching and rate limiting
- health checks, metrics, and a basic circuit breaker

## Requirements

- Docker and Docker Compose

## Local Setup

Start the full stack:

```bash
docker compose up --build
```

The API will be available at:

```text
http://127.0.0.1:3000
```

If you want to recreate the database from scratch:

```bash
docker compose down -v
docker compose up --build
```

## Main Environment Variables

The most important variables are:

- `SERVICE_HOST`
- `SERVICE_PORT`
- `DATABASE_URL`
- `REDIS_URL`
- `WRITE_RATE_LIMIT_PER_MINUTE`
- `READ_RATE_LIMIT_PER_MINUTE`
- `CIRCUIT_BREAKER_FAILURE_THRESHOLD`
- `CIRCUIT_BREAKER_OPEN_SECONDS`

They are already configured for local development in `docker-compose.yml`.

## Tests

To run the local integration test suite:

```bash
DATABASE_URL=postgresql://postgres:postgres@localhost:5432/app \
REDIS_URL=redis://127.0.0.1:6379/ \
cargo test -- --test-threads=1
```

## Main Endpoints

Health and observability:

- `GET /health/live`
- `GET /health/ready`
- `GET /metrics`

Likes:

- `POST /v1/likes`
- `DELETE /v1/likes/{content_type}/{content_id}`
- `GET /v1/likes/{content_type}/{content_id}/count`
- `GET /v1/likes/{content_type}/{content_id}/status`
- `POST /v1/likes/batch/counts`
- `POST /v1/likes/batch/statuses`
- `GET /v1/likes/user`
- `GET /v1/likes/top`
- `GET /v1/likes/stream`

Internal mocks used by the service itself:

- `GET /v1/auth/validate`
- `GET /v1/{content_type}/{content_id}`

## Quick Examples

Create a like:

```bash
curl -i -X POST http://127.0.0.1:3000/v1/likes \
  -H 'Authorization: Bearer valid-alice-token' \
  -H 'content-type: application/json' \
  -d '{"content_type":"post","content_id":"aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaa1"}'
```

Get the count:

```bash
curl -i http://127.0.0.1:3000/v1/likes/post/aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaa1/count
```

Get top likes:

```bash
curl -i 'http://127.0.0.1:3000/v1/likes/top?window=all&limit=10'
```

Open the SSE stream:

```bash
curl -N 'http://127.0.0.1:3000/v1/likes/stream?content_type=post&content_id=aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaa1'
```

## Design Notes

- Postgres is the source of truth for `likes` and `like_counts`.
- Redis is used for count caching and rate limiting.
- If Redis is unavailable on the read path, the service falls back to Postgres.
- The leaderboard is intentionally simple in this phase: functional and reviewable, but not optimized for high scale.
- The SSE endpoint is currently implemented with an in-memory event bus, which is suitable for a single instance but not yet for multi-instance fanout.
- The service includes internal Profile API and Content API mocks to simplify local development and integration.
