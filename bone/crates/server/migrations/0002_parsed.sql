CREATE TABLE parsed (
    id INTEGER PRIMARY KEY,
    blob TEXT NOT NULL UNIQUE,
    version INTEGER NOT NULL,
    message_id TEXT NOT NULL,
    thread INTEGER NOT NULL DEFAULT 0,
    subject TEXT NOT NULL,
    from_name TEXT,
    from_addr TEXT,
    to_json TEXT NOT NULL,
    cc_json TEXT NOT NULL,
    date INTEGER,
    recipient TEXT,
    attachments_json TEXT NOT NULL,
    snippet TEXT NOT NULL
);

CREATE INDEX parsed_thread ON parsed (thread);
CREATE INDEX parsed_message_id ON parsed (message_id);

CREATE TABLE thread_ref (
    msgid TEXT PRIMARY KEY,
    thread INTEGER NOT NULL
);

CREATE INDEX thread_ref_thread ON thread_ref (thread);

CREATE VIRTUAL TABLE search USING fts5 (
    subject,
    sender,
    recipients,
    body,
    attachments,
    content = '',
    contentless_delete = 1,
    tokenize = 'unicode61 remove_diacritics 2'
);
