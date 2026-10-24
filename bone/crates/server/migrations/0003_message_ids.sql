CREATE TABLE message_new (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    folder_id INTEGER NOT NULL REFERENCES folder (id) ON DELETE CASCADE,
    uid INTEGER NOT NULL,
    modseq INTEGER NOT NULL,
    flags TEXT NOT NULL,
    internal_date INTEGER NOT NULL,
    size INTEGER NOT NULL,
    blob TEXT,
    UNIQUE (folder_id, uid)
);

INSERT INTO message_new (id, folder_id, uid, modseq, flags, internal_date, size, blob)
SELECT rowid, folder_id, uid, modseq, flags, internal_date, size, blob FROM message;

DROP TABLE message;

ALTER TABLE message_new RENAME TO message;

CREATE INDEX message_blob ON message (blob);
