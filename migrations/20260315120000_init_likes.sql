-- likes is the relational source of truth: one row means one user liked one content item.
CREATE TABLE likes (
    user_id TEXT NOT NULL,
    content_type TEXT NOT NULL,
    content_id TEXT NOT NULL,
    liked_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (user_id, content_type, content_id)
);

-- Supports listing a user's likes ordered by most recent first.
CREATE INDEX idx_likes_user_liked_at
    ON likes (user_id, liked_at DESC);

-- like_counts stores the persisted aggregate count per content item.
CREATE TABLE like_counts (
    content_type TEXT NOT NULL,
    content_id TEXT NOT NULL,
    like_count BIGINT NOT NULL DEFAULT 0,
    PRIMARY KEY (content_type, content_id),
    CONSTRAINT like_counts_non_negative CHECK (like_count >= 0)
);
