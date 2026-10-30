CREATE TABLE folder (
    id INTEGER PRIMARY KEY,
    account TEXT NOT NULL,
    path TEXT NOT NULL,
    role TEXT,
    uidvalidity INTEGER,
    uidnext INTEGER NOT NULL DEFAULT 1,
    highestmodseq INTEGER NOT NULL DEFAULT 0,
    UNIQUE (account, path)
);

CREATE TABLE message (
    folder_id INTEGER NOT NULL REFERENCES folder (id) ON DELETE CASCADE,
    uid INTEGER NOT NULL,
    modseq INTEGER NOT NULL,
    flags TEXT NOT NULL,
    internal_date INTEGER NOT NULL,
    size INTEGER NOT NULL,
    blob TEXT,
    PRIMARY KEY (folder_id, uid)
);

CREATE INDEX message_blob ON message (blob);
