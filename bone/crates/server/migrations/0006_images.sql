CREATE TABLE image (
    id TEXT PRIMARY KEY,
    url TEXT NOT NULL,
    content_type TEXT,
    size INTEGER,
    failed_at INTEGER,
    used_at INTEGER NOT NULL
);

CREATE INDEX image_used_at ON image (used_at);
