CREATE TABLE like_hourly_counts (
    bucket_start TIMESTAMPTZ NOT NULL,
    content_type TEXT NOT NULL,
    content_id TEXT NOT NULL,
    like_count BIGINT NOT NULL DEFAULT 0,
    PRIMARY KEY (bucket_start, content_type, content_id),
    CONSTRAINT like_hourly_counts_non_negative CHECK (like_count >= 0)
);

CREATE INDEX idx_like_hourly_counts_content_bucket
    ON like_hourly_counts (content_type, bucket_start DESC);

INSERT INTO like_hourly_counts (bucket_start, content_type, content_id, like_count)
SELECT
    date_trunc('hour', liked_at AT TIME ZONE 'UTC') AT TIME ZONE 'UTC' AS bucket_start,
    content_type,
    content_id,
    COUNT(*)::bigint AS like_count
FROM likes
GROUP BY 1, 2, 3;
