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

There are two supported local workflows:

1. full stack in Docker
2. dependencies in Docker + app running on the host with `cargo run --release`

Start the full stack:

```bash
docker compose up --build
```

The API will be available at:

```text
http://127.0.0.1:8080
```

If you want to recreate the database from scratch:

```bash
docker compose down -v
docker compose up --build
```

The Postgres schema is created from the versioned SQL files in `migrations/`.
The application expects those migrations to be applied before startup and only validates that the required tables exist.

For host-based local runs, stop the Dockerized `social-api` first if you already started the full stack, then copy `.env.example` to `.env` and adjust values if needed:

```bash
docker compose stop social-api
cp .env.example .env
docker compose up -d postgres redis mock-profile-api mock-post-api mock-bonus-hunter-api mock-top-picks-api
cargo run --release
```

The service loads `.env` automatically when present, without overriding environment variables that are already exported by the shell.

## Main Environment Variables

Primary variables used by this service:

- `SERVICE_HOST`
- `HTTP_PORT`
- `DATABASE_URL`
- `READ_DATABASE_URL`
- `REDIS_URL`
- `LOG_LEVEL`
- `RUST_LOG`
- `DB_MAX_CONNECTIONS`
- `DB_MIN_CONNECTIONS`
- `DB_ACQUIRE_TIMEOUT_SECS`
- `REDIS_POOL_SIZE`
- `RATE_LIMIT_WRITE_PER_MINUTE`
- `RATE_LIMIT_READ_PER_MINUTE`
- `CACHE_TTL_LIKE_COUNTS_SECS`
- `CACHE_TTL_CONTENT_VALIDATION_SECS`
- `CACHE_TTL_USER_STATUS_SECS`
- `CIRCUIT_BREAKER_FAILURE_THRESHOLD`
- `CIRCUIT_BREAKER_RECOVERY_TIMEOUT_SECS`
- `CIRCUIT_BREAKER_SUCCESS_THRESHOLD`
- `CIRCUIT_BREAKER_FAILURE_WINDOW_SECONDS`
- `SHUTDOWN_TIMEOUT_SECS`
- `SSE_HEARTBEAT_INTERVAL_SECS`
- `LEADERBOARD_REFRESH_INTERVAL_SECS`
- `PROFILE_API_URL`
- `CONTENT_API_REGISTRY`

They are already configured for local development in `docker-compose.yml`, and `.env.example` mirrors the same setup for host-based runs.
The service fails fast on startup if required connection settings or external dependency URLs are missing.
`CACHE_TTL_USER_STATUS_SECS` controls the Redis TTL for authenticated like-status responses, and `LEADERBOARD_REFRESH_INTERVAL_SECS` controls the Redis TTL for cached leaderboard responses.
`CONTENT_API_REGISTRY` is the source of truth for content validation routing, for example `post=http://127.0.0.1:8081,bonus_hunter=http://127.0.0.1:8082,top_picks=http://127.0.0.1:8083,news_article=http://127.0.0.1:8085`.

## Tests

With local Postgres and Redis available, run the integration test suite with:

```bash
DATABASE_URL=postgresql://postgres:postgres@localhost:5432/likes_service \
REDIS_URL=redis://127.0.0.1:6379/ \
cargo test -- --test-threads=1
```

If you do not already have Postgres and Redis running locally, the simplest setup is:

```bash
docker compose up -d postgres redis
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

The Docker setup now runs the mocks as separate services in Compose:
- `social-api`
- `postgres`
- `redis`
- `mock-profile-api`
- `mock-post-api`
- `mock-bonus-hunter-api`
- `mock-top-picks-api`

The three content mock services share the same executable and mock implementation, but run as separate Compose services to match the challenge topology more closely.

## Quick Examples

Check liveness and readiness:

```bash
curl -i http://127.0.0.1:8080/health/live
curl -i http://127.0.0.1:8080/health/ready
```

Create a like:

```bash
curl -i -X POST http://127.0.0.1:8080/v1/likes \
  -H 'Authorization: Bearer tok_user_1' \
  -H 'content-type: application/json' \
  -d '{"content_type":"post","content_id":"731b0395-4888-4822-b516-05b4b7bf2089"}'
```

Get the count:

```bash
curl -i http://127.0.0.1:8080/v1/likes/post/731b0395-4888-4822-b516-05b4b7bf2089/count
```

Get the authenticated status:

```bash
curl -i http://127.0.0.1:8080/v1/likes/post/731b0395-4888-4822-b516-05b4b7bf2089/status \
  -H 'Authorization: Bearer tok_user_1'
```

Delete a like:

```bash
curl -i -X DELETE http://127.0.0.1:8080/v1/likes/post/731b0395-4888-4822-b516-05b4b7bf2089 \
  -H 'Authorization: Bearer tok_user_1'
```

Get top likes:

```bash
curl -i 'http://127.0.0.1:8080/v1/likes/top?window=all&limit=10'
```

Filtered top likes:

```bash
curl -i 'http://127.0.0.1:8080/v1/likes/top?content_type=post&window=7d&limit=10'
```

Open the SSE stream:

```bash
curl -N 'http://127.0.0.1:8080/v1/likes/stream?content_type=post&content_id=731b0395-4888-4822-b516-05b4b7bf2089'
```

## Pagination

`GET /v1/likes/user` uses cursor-based pagination rather than offset-based pagination.

- The cursor is an opaque base64 value built from `liked_at` and `content_id`.
- This fits the endpoint ordering, which is "most recent likes first".
- Cursor pagination avoids the instability and skipped/duplicated rows that offset pagination can produce when new likes are inserted while a client is paging.
- The trade-off is that the cursor is tied to the current sort order and should be treated as an opaque token by clients.

## Architecture Summary

- Postgres is the source of truth for `likes`, `like_counts`, and hourly leaderboard aggregates.
- Redis is used for count caching, rate limiting, and SSE event fanout through Pub/Sub.
- Content validation is resolved through a registry of `(content_type -> base_url)` definitions loaded from configuration. New content types can be introduced without code changes when using `CONTENT_API_REGISTRY`.
- The repository includes local mock implementations for Profile API and Content API, and Compose runs them as separate services for local integration testing.
- For local convenience, the main app router still exposes equivalent mock endpoints too, which keeps single-process runs simple at the cost of some extra surface area.
- The container image runs as a non-root user and exposes a liveness `HEALTHCHECK` against `/health/live`.

## Trade-Offs

- Count reads use Redis as a cache, but fall back to Postgres when Redis is unavailable. This keeps the read path available at the cost of degraded performance.
- Content validation results are cached in Redis with a bounded TTL. Both `200` and `404` outcomes are cacheable, which reduces repeated dependency calls for hot content IDs.
- Count cache entries use a bounded TTL and are also refreshed opportunistically on writes. In practice this keeps the maximum staleness window bounded by `CACHE_TTL_LIKE_COUNTS_SECS`, while still healing naturally after a Redis restart or cold start.
- Count cache entries are also pushed to each instance over Redis Pub/Sub after `like` / `unlike`, so hot `GET /count` requests can usually hit the per-process L1 cache instead of paying a Redis roundtrip on every read.
- Count cache repopulation uses a simple single-flight strategy per key. When a hot key expires, only one request repopulates it from Postgres while concurrent requests wait for the refreshed value instead of stampeding the database.
- Public read endpoints expose HTTP cache validators. `GET /v1/likes/{type}/{id}/count` and `GET /v1/likes/top` return `ETag` and `Cache-Control` headers and support `If-None-Match` / `304 Not Modified`, which makes them friendlier to browsers, CDNs, and reverse proxies.
- Authenticated like-status responses are cached in Redis for `CACHE_TTL_USER_STATUS_SECS`, and `like` / `unlike` refresh that cache directly after writes.
- `GET /v1/likes/top` uses persisted totals for `window=all` and hourly preaggregation for `24h`, `7d`, and `30d`. This is simpler and more scalable than aggregating directly from `likes` on every request, while staying easier to review than a more advanced pipeline.
- `GET /v1/likes/top` responses are cached in Redis per `(window, content_type, limit)` combination for `LEADERBOARD_REFRESH_INTERVAL_SECS`.
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
- Introduce stronger migration tooling beyond Docker init scripts if the service needs a more formal deployment workflow.

## Extras

OpenAPI:
- The HTTP contract is documented in [openapi.yaml](openapi.yaml).
- Swagger UI is served at `/docs` and uses `/openapi.yaml` from the same origin.

HTTP cache validators:
- Public read endpoints `GET /v1/likes/{type}/{id}/count` and `GET /v1/likes/top` return `ETag` and `Cache-Control`.
- Both endpoints support `If-None-Match` and can answer `304 Not Modified`, which makes them friendlier to browsers, CDNs, and reverse proxies.

gRPC:
- The repository includes [proto/likes.proto](proto/likes.proto) and an optional tonic-based gRPC server for the same likes domain.
- The gRPC server reuses the same domain logic and persistence layer as the HTTP API. It starts when `GRPC_PORT` is present in the environment.
- In Docker Compose it is not enabled by default; in host-based local runs it is enabled by the provided `.env.example` unless you remove `GRPC_PORT`.
- Server reflection is enabled, so `grpcurl` can inspect services and methods without passing the local proto file.
- Supported methods: `Like`, `Unlike`, `GetLikeCount`, `GetLikeStatus`, `GetUserLikes`, `BatchGetLikeCounts`, `BatchGetLikeStatuses`, and `GetTopLikes`.
- Example host-based local run:

```bash
cp .env.example .env
docker compose stop social-api
docker compose up -d postgres redis mock-profile-api mock-post-api mock-bonus-hunter-api mock-top-picks-api
cargo run --release
```

Then, for example:

```bash
grpcurl -plaintext 127.0.0.1:50051 list
```

k6 load testing:
- The repository includes [k6/load-test.js](k6/load-test.js) to validate the challenge hot paths.
- Supported modes:
  - `BENCHMARK=read` for `GET /v1/likes/{type}/{id}/count`
  - `BENCHMARK=batch` for `POST /v1/likes/batch/counts`
  - `BENCHMARK=write` for `POST /v1/likes`
  - `BENCHMARK=mixed` for an `80/15/5` read/batch/write mix
- Recommended local setup:
  - keep Postgres and Redis in Docker
  - run the app as a local `--release` binary
  - benchmark with `LOG_LEVEL=warn` and `RUST_LOG=warn`

Example:

```bash
cp .env.example .env
docker compose stop social-api
docker compose up -d postgres redis mock-profile-api mock-post-api mock-bonus-hunter-api mock-top-picks-api
LOG_LEVEL=warn \
RUST_LOG=warn \
cargo run --release
```

```bash
BASE_URL=http://127.0.0.1:8080 BENCHMARK=read k6 run k6/load-test.js
BASE_URL=http://127.0.0.1:8080 BENCHMARK=batch k6 run k6/load-test.js
BASE_URL=http://127.0.0.1:8080 BENCHMARK=write k6 run k6/load-test.js
BASE_URL=http://127.0.0.1:8080 BENCHMARK=mixed MIXED_RATE=6666 k6 run k6/load-test.js
```

Notes:
- The script is `RATE_LIMIT_AWARE=true` by default, so it avoids turning the benchmark into a pure rate-limit exercise.
- `mixed` is a traffic ratio in the challenge, not a fixed total throughput target.
- `MIXED_RATE=6666` keeps the `15%` batch share close to the standalone `1,000 rps` batch target.
- The `read` benchmark is measured in steady state. The k6 setup warms the hot `count` keys and the synthetic read limiter leases before the timed run starts, so the result reflects the hot path rather than cold cache fill or first-use coordination costs.
- The per-process L1 cache for `GET /count` uses a long safety TTL and is refreshed push-style after writes, which keeps the hot path stable while still allowing recovery after cache loss or restarts.
- In repeated local steady-state runs, the `read` benchmark at `10k rps` typically landed around `~3.5-5.3ms p99` with the app running as a host `--release` binary and `LOG_LEVEL=warn`, although occasional local-machine outliers still appeared in some runs.

## License

MIT. See [LICENSE](LICENSE).
