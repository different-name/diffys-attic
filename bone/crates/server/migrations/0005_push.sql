CREATE TABLE push_subscription (
    endpoint TEXT PRIMARY KEY,
    p256dh TEXT NOT NULL,
    auth TEXT NOT NULL,
    created_at INTEGER NOT NULL
);

CREATE TABLE notified (
    message_id TEXT PRIMARY KEY,
    at INTEGER NOT NULL
);
