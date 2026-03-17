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
DATABASE_URL=postgresql://postgres:postgres@localhost:5432/likes_service \
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

Filtered top likes:

```bash
curl -i 'http://127.0.0.1:3000/v1/likes/top?content_type=post&window=7d&limit=10'
```

Open the SSE stream:

```bash
curl -N 'http://127.0.0.1:3000/v1/likes/stream?content_type=post&content_id=aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaa1'
```

## Architecture Summary

- Postgres is the source of truth for `likes`, `like_counts`, and hourly leaderboard aggregates.
- Redis is used for count caching, rate limiting, and SSE event fanout through Pub/Sub.
- Profile API and Content API are represented by internal mock endpoints so the service can be run and tested locally as a single process.

## Trade-Offs

- Count reads use Redis as a cache, but fall back to Postgres when Redis is unavailable. This keeps the read path available at the cost of degraded performance.
- Content validation results are cached in Redis with a bounded TTL. Both `200` and `404` outcomes are cacheable, which reduces repeated dependency calls for hot content IDs.
- `GET /v1/likes/top` uses persisted totals for `window=all` and hourly preaggregation for `24h`, `7d`, and `30d`. This is simpler and more scalable than aggregating directly from `likes` on every request, while staying easier to review than a more advanced pipeline.
- `GET /v1/likes/stream` uses Redis Pub/Sub. This makes the stream work across instances that share Redis, without introducing a heavier event system.
- Circuit breakers and rate limiters are intentionally simple. They provide operational protection and visibility without adding too much state-machine complexity to the codebase.

## Known Limitations

- Leaderboard aggregation is still relatively simple. A higher-scale version would likely use background materialization, retention policies for old buckets, and more explicit indexing and query tuning.
- Redis Pub/Sub does not provide replay or durable event delivery. It is a good fit for live SSE fanout, but not for event history.
- Content validation cache entries can be stale for up to the configured TTL. This keeps the service simple and fast, but it means external content removals are not reflected immediately.
- The project is optimized for clarity and challenge delivery, not for multi-region deployment or very high write throughput.

## Next Scaling Steps

- Add retention and compaction strategy for old hourly leaderboard buckets.
- Move health, topology, and trade-off notes into more explicit operational documentation if the service becomes long-lived.
- Introduce stronger migration management for schema evolution beyond Docker init scripts and startup schema checks.
