use std::collections::HashSet;
use std::path::Path;
use std::str::FromStr;

use anyhow::{Context, Result};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use sqlx::{Row, SqlitePool};

use crate::parse::{Mailbox, Parsed};
use crate::push::Subscription;

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

#[derive(Clone)]
pub struct Db {
    pub(crate) pool: SqlitePool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Folder {
    pub id: i64,
    pub path: String,
    pub role: Option<String>,
    pub uidvalidity: Option<u32>,
    pub uidnext: u32,
    pub highestmodseq: u64,
}

#[derive(Debug, Clone)]
pub struct NewMessage {
    pub uid: u32,
    pub modseq: u64,
    pub flags: String,
    pub internal_date: i64,
    pub size: u32,
}

impl Db {
    pub async fn open(path: &Path) -> Result<Self> {
        if !path.exists() {
            use std::os::unix::fs::OpenOptionsExt;
            std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(false)
                .mode(0o600)
                .open(path)
                .with_context(|| format!("creating {}", path.display()))?;
        }
        let opts = SqliteConnectOptions::from_str(&format!("sqlite://{}", path.display()))?
            .journal_mode(SqliteJournalMode::Wal)
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(4)
            .connect_with(opts)
            .await
            .with_context(|| format!("opening {}", path.display()))?;
        Self::setup(pool).await
    }

    #[cfg(test)]
    pub async fn memory() -> Result<Self> {
        let opts = SqliteConnectOptions::from_str("sqlite::memory:")?.foreign_keys(true);
        // one connection, every new in-memory connection would be a fresh empty database
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(opts)
            .await?;
        Self::setup(pool).await
    }

    async fn setup(pool: SqlitePool) -> Result<Self> {
        MIGRATOR.run(&pool).await.context("running migrations")?;
        Ok(Self { pool })
    }

    pub async fn folders(&self, account: &str) -> Result<Vec<Folder>> {
        let rows = sqlx::query(
            "SELECT id, path, role, uidvalidity, uidnext, highestmodseq FROM folder WHERE account = ? ORDER BY path",
        )
        .bind(account)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .iter()
            .map(|r| Folder {
                id: r.get("id"),
                path: r.get("path"),
                role: r.get("role"),
                uidvalidity: r.get::<Option<i64>, _>("uidvalidity").map(|v| v as u32),
                uidnext: r.get::<i64, _>("uidnext") as u32,
                highestmodseq: r.get::<i64, _>("highestmodseq") as u64,
            })
            .collect())
    }

    pub async fn upsert_folder(&self, account: &str, path: &str, role: Option<&str>) -> Result<()> {
        sqlx::query(
            "INSERT INTO folder (account, path, role) VALUES (?, ?, ?)
             ON CONFLICT (account, path) DO UPDATE SET role = excluded.role",
        )
        .bind(account)
        .bind(path)
        .bind(role)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn drop_folder(&self, id: i64) -> Result<()> {
        sqlx::query("DELETE FROM folder WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn reset_folder(&self, id: i64, uidvalidity: u32) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("DELETE FROM message WHERE folder_id = ?")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            "UPDATE folder SET uidvalidity = ?, uidnext = 1, highestmodseq = 0 WHERE id = ?",
        )
        .bind(uidvalidity as i64)
        .bind(id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn set_folder_state(&self, id: i64, uidnext: u32, highestmodseq: u64) -> Result<()> {
        sqlx::query("UPDATE folder SET uidnext = ?, highestmodseq = ? WHERE id = ?")
            .bind(uidnext as i64)
            .bind(highestmodseq as i64)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn insert_messages(&self, folder_id: i64, messages: &[NewMessage]) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        for m in messages {
            sqlx::query(
                "INSERT INTO message (folder_id, uid, modseq, flags, internal_date, size) VALUES (?, ?, ?, ?, ?, ?)
                 ON CONFLICT (folder_id, uid) DO UPDATE SET modseq = excluded.modseq, flags = excluded.flags",
            )
            .bind(folder_id)
            .bind(m.uid as i64)
            .bind(m.modseq as i64)
            .bind(&m.flags)
            .bind(m.internal_date)
            .bind(m.size as i64)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn update_flags(&self, folder_id: i64, changes: &[(u32, u64, String)]) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        for (uid, modseq, flags) in changes {
            sqlx::query("UPDATE message SET modseq = ?, flags = ? WHERE folder_id = ? AND uid = ?")
                .bind(*modseq as i64)
                .bind(flags)
                .bind(folder_id)
                .bind(*uid as i64)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn count(&self, folder_id: i64) -> Result<u32> {
        let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM message WHERE folder_id = ?")
            .bind(folder_id)
            .fetch_one(&self.pool)
            .await?;
        Ok(n as u32)
    }

    pub async fn uids(&self, folder_id: i64) -> Result<Vec<u32>> {
        let uids: Vec<i64> =
            sqlx::query_scalar("SELECT uid FROM message WHERE folder_id = ? ORDER BY uid")
                .bind(folder_id)
                .fetch_all(&self.pool)
                .await?;
        Ok(uids.into_iter().map(|u| u as u32).collect())
    }

    pub async fn remove_messages(&self, folder_id: i64, uids: &[u32]) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        for uid in uids {
            sqlx::query("DELETE FROM message WHERE folder_id = ? AND uid = ?")
                .bind(folder_id)
                .bind(*uid as i64)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn missing_blobs(&self, folder_id: i64, limit: u32) -> Result<Vec<u32>> {
        let uids: Vec<i64> = sqlx::query_scalar(
            "SELECT uid FROM message WHERE folder_id = ? AND blob IS NULL ORDER BY uid DESC LIMIT ?",
        )
        .bind(folder_id)
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await?;
        Ok(uids.into_iter().map(|u| u as u32).collect())
    }

    pub async fn next_missing(&self, account: &str, limit: u32) -> Result<Option<(i64, Vec<u32>)>> {
        let folder: Option<i64> = sqlx::query_scalar(
            "SELECT m.folder_id FROM message m JOIN folder f ON f.id = m.folder_id
             WHERE f.account = ? AND m.blob IS NULL ORDER BY m.internal_date DESC LIMIT 1",
        )
        .bind(account)
        .fetch_optional(&self.pool)
        .await?;
        match folder {
            Some(id) => Ok(Some((id, self.missing_blobs(id, limit).await?))),
            None => Ok(None),
        }
    }

    pub async fn set_blob(&self, folder_id: i64, uid: u32, blob: &str) -> Result<()> {
        sqlx::query("UPDATE message SET blob = ? WHERE folder_id = ? AND uid = ?")
            .bind(blob)
            .bind(folder_id)
            .bind(uid as i64)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn referenced_blobs(&self) -> Result<HashSet<String>> {
        let blobs: Vec<String> =
            sqlx::query_scalar("SELECT DISTINCT blob FROM message WHERE blob IS NOT NULL")
                .fetch_all(&self.pool)
                .await?;
        Ok(blobs.into_iter().collect())
    }
}

impl Db {
    pub async fn unparsed(&self, version: i64, limit: u32) -> Result<Vec<String>> {
        Ok(sqlx::query_scalar(
            "SELECT DISTINCT m.blob FROM message m LEFT JOIN parsed p ON p.blob = m.blob
             WHERE m.blob IS NOT NULL AND (p.blob IS NULL OR p.version < ?) LIMIT ?",
        )
        .bind(version)
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await?)
    }

    pub async fn store_parsed(&self, version: i64, items: &[(String, Parsed)]) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        for (blob, p) in items {
            let id: i64 = sqlx::query_scalar(
                "INSERT INTO parsed (blob, version, message_id, subject, from_name, from_addr, to_json, cc_json, date, recipient, attachments_json, snippet, recipient_hosted, reply_to_json, parents_json)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                 ON CONFLICT (blob) DO UPDATE SET version = excluded.version, message_id = excluded.message_id,
                    subject = excluded.subject, from_name = excluded.from_name, from_addr = excluded.from_addr,
                    to_json = excluded.to_json, cc_json = excluded.cc_json, date = excluded.date,
                    recipient = excluded.recipient, attachments_json = excluded.attachments_json, snippet = excluded.snippet,
                    recipient_hosted = excluded.recipient_hosted, reply_to_json = excluded.reply_to_json, parents_json = excluded.parents_json
                 RETURNING id",
            )
            .bind(blob)
            .bind(version)
            .bind(&p.message_id)
            .bind(&p.subject)
            .bind(p.from.as_ref().and_then(|f| f.name.as_deref()))
            .bind(p.from.as_ref().map(|f| f.addr.as_str()))
            .bind(serde_json::to_string(&p.to)?)
            .bind(serde_json::to_string(&p.cc)?)
            .bind(p.date)
            .bind(p.recipient.as_deref())
            .bind(serde_json::to_string(&p.attachments)?)
            .bind(&p.snippet)
            .bind(p.recipient_hosted)
            .bind(serde_json::to_string(&p.reply_to)?)
            .bind(serde_json::to_string(&p.parents)?)
            .fetch_one(&mut *tx)
            .await?;

            let mut threads: Vec<i64> = Vec::new();
            for msgid in p.thread_ids() {
                let t: Option<i64> =
                    sqlx::query_scalar("SELECT thread FROM thread_ref WHERE msgid = ?")
                        .bind(msgid)
                        .fetch_optional(&mut *tx)
                        .await?;
                threads.extend(t);
            }
            let target = threads.iter().copied().min().unwrap_or(id);
            for t in threads.iter().copied().filter(|t| *t != target) {
                sqlx::query("UPDATE parsed SET thread = ? WHERE thread = ?")
                    .bind(target)
                    .bind(t)
                    .execute(&mut *tx)
                    .await?;
                sqlx::query("UPDATE thread_ref SET thread = ? WHERE thread = ?")
                    .bind(target)
                    .bind(t)
                    .execute(&mut *tx)
                    .await?;
            }
            sqlx::query("UPDATE parsed SET thread = ? WHERE id = ?")
                .bind(target)
                .bind(id)
                .execute(&mut *tx)
                .await?;
            for msgid in p.thread_ids() {
                sqlx::query("INSERT OR REPLACE INTO thread_ref (msgid, thread) VALUES (?, ?)")
                    .bind(msgid)
                    .bind(target)
                    .execute(&mut *tx)
                    .await?;
            }

            sqlx::query("DELETE FROM search WHERE rowid = ?")
                .bind(id)
                .execute(&mut *tx)
                .await?;
            sqlx::query(
                "INSERT INTO search (rowid, subject, sender, recipients, body, attachments) VALUES (?, ?, ?, ?, ?, ?)",
            )
            .bind(id)
            .bind(&p.subject)
            .bind(p.from.as_ref().map(search_text).unwrap_or_default())
            .bind(
                p.to.iter()
                    .chain(&p.cc)
                    .map(search_text)
                    .chain(p.recipient.clone())
                    .collect::<Vec<_>>()
                    .join(" "),
            )
            .bind(&p.body)
            .bind(
                p.attachments
                    .iter()
                    .map(|a| a.name.as_str())
                    .collect::<Vec<_>>()
                    .join(" "),
            )
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn prune_parsed(&self) -> Result<u64> {
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "DELETE FROM search WHERE rowid IN (SELECT id FROM parsed WHERE blob NOT IN
             (SELECT blob FROM message WHERE blob IS NOT NULL))",
        )
        .execute(&mut *tx)
        .await?;
        let removed = sqlx::query(
            "DELETE FROM parsed WHERE blob NOT IN (SELECT blob FROM message WHERE blob IS NOT NULL)",
        )
        .execute(&mut *tx)
        .await?
        .rows_affected();
        tx.commit().await?;
        Ok(removed)
    }
}

fn search_text(m: &Mailbox) -> String {
    match &m.name {
        Some(name) => format!("{name} {}", m.addr),
        None => m.addr.clone(),
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Moved {
    pub internal_date: i64,
    pub size: u32,
    pub blob: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Located {
    pub account: String,
    pub folder_id: i64,
    pub uid: u32,
}

impl Db {
    pub async fn edit_flags(
        &self,
        folder_id: i64,
        uids: &[u32],
        add: &[&str],
        remove: &[&str],
    ) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        for uid in uids {
            let flags: Option<String> =
                sqlx::query_scalar("SELECT flags FROM message WHERE folder_id = ? AND uid = ?")
                    .bind(folder_id)
                    .bind(*uid as i64)
                    .fetch_optional(&mut *tx)
                    .await?;
            if let Some(flags) = flags {
                sqlx::query("UPDATE message SET flags = ? WHERE folder_id = ? AND uid = ?")
                    .bind(crate::actions::edit_flags(&flags, add, remove))
                    .bind(folder_id)
                    .bind(*uid as i64)
                    .execute(&mut *tx)
                    .await?;
            }
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn moved(&self, folder_id: i64, uids: &[u32]) -> Result<Vec<Moved>> {
        let mut out = Vec::new();
        for uid in uids {
            let row = sqlx::query(
                "SELECT internal_date, size, blob FROM message WHERE folder_id = ? AND uid = ? AND blob IS NOT NULL",
            )
            .bind(folder_id)
            .bind(*uid as i64)
            .fetch_optional(&self.pool)
            .await?;
            if let Some(r) = row {
                out.push(Moved {
                    internal_date: r.get("internal_date"),
                    size: r.get::<i64, _>("size") as u32,
                    blob: r.get("blob"),
                });
            }
        }
        Ok(out)
    }

    // MOVE keeps INTERNALDATE and size, so a unique match is the same message
    pub async fn adopt_blobs(&self, folder_id: i64, moved: &[Moved]) -> Result<usize> {
        let mut adopted = 0;
        for m in moved {
            let uids: Vec<i64> = sqlx::query_scalar(
                "SELECT uid FROM message WHERE folder_id = ? AND blob IS NULL AND internal_date = ? AND size = ?",
            )
            .bind(folder_id)
            .bind(m.internal_date)
            .bind(m.size as i64)
            .fetch_all(&self.pool)
            .await?;
            if let [uid] = uids[..] {
                self.set_blob(folder_id, uid as u32, &m.blob).await?;
                adopted += 1;
            }
        }
        Ok(adopted)
    }

    pub async fn without_blob(&self, folder_id: i64, uids: &[u32]) -> Result<Vec<u32>> {
        let mut out = Vec::new();
        for uid in uids {
            let missing: Option<i64> = sqlx::query_scalar(
                "SELECT uid FROM message WHERE folder_id = ? AND uid = ? AND blob IS NULL",
            )
            .bind(folder_id)
            .bind(*uid as i64)
            .fetch_optional(&self.pool)
            .await?;
            out.extend(missing.map(|u| u as u32));
        }
        Ok(out)
    }

    pub async fn locate(&self, ids: &[i64]) -> Result<Vec<Located>> {
        let mut out = Vec::new();
        for id in ids {
            let row = sqlx::query(
                "SELECT f.account, m.folder_id, m.uid FROM message m JOIN folder f ON f.id = m.folder_id WHERE m.id = ?",
            )
            .bind(*id)
            .fetch_optional(&self.pool)
            .await?;
            if let Some(r) = row {
                out.push(Located {
                    account: r.get("account"),
                    folder_id: r.get("folder_id"),
                    uid: r.get::<i64, _>("uid") as u32,
                });
            }
        }
        Ok(out)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Fresh {
    pub id: i64,
    pub message_id: String,
    pub from: String,
    pub subject: String,
    pub recipient: Option<String>,
}

impl Db {
    pub async fn fresh_inbox(&self, blobs: &[String], since: i64) -> Result<Vec<Fresh>> {
        let mut out = Vec::new();
        for blob in blobs {
            let rows = sqlx::query(
                "SELECT m.id, p.message_id, COALESCE(p.from_name, p.from_addr, '') AS sender, p.subject, p.recipient
                 FROM message m JOIN folder f ON f.id = m.folder_id JOIN parsed p ON p.blob = m.blob
                 WHERE m.blob = ? AND f.role = 'inbox' AND m.internal_date >= ?
                   AND (' ' || m.flags || ' ') NOT LIKE '% \\Seen %'",
            )
            .bind(blob)
            .bind(since)
            .fetch_all(&self.pool)
            .await?;
            out.extend(rows.iter().map(|r| Fresh {
                id: r.get("id"),
                message_id: r.get("message_id"),
                from: r.get("sender"),
                subject: r.get("subject"),
                recipient: r.get("recipient"),
            }));
        }
        Ok(out)
    }

    pub async fn mark_notified(&self, message_id: &str, at: i64) -> Result<bool> {
        let done = sqlx::query("INSERT OR IGNORE INTO notified (message_id, at) VALUES (?, ?)")
            .bind(message_id)
            .bind(at)
            .execute(&self.pool)
            .await?;
        Ok(done.rows_affected() == 1)
    }

    pub async fn prune_notified(&self, before: i64) -> Result<u64> {
        Ok(sqlx::query("DELETE FROM notified WHERE at < ?")
            .bind(before)
            .execute(&self.pool)
            .await?
            .rows_affected())
    }

    pub async fn add_subscription(&self, sub: &Subscription, at: i64) -> Result<()> {
        sqlx::query(
            "INSERT INTO push_subscription (endpoint, p256dh, auth, created_at) VALUES (?, ?, ?, ?)
             ON CONFLICT (endpoint) DO UPDATE SET p256dh = excluded.p256dh, auth = excluded.auth",
        )
        .bind(&sub.endpoint)
        .bind(&sub.p256dh)
        .bind(&sub.auth)
        .bind(at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn remove_subscription(&self, endpoint: &str) -> Result<bool> {
        Ok(
            sqlx::query("DELETE FROM push_subscription WHERE endpoint = ?")
                .bind(endpoint)
                .execute(&self.pool)
                .await?
                .rows_affected()
                == 1,
        )
    }

    pub async fn subscriptions(&self) -> Result<Vec<Subscription>> {
        let rows = sqlx::query("SELECT endpoint, p256dh, auth FROM push_subscription")
            .fetch_all(&self.pool)
            .await?;
        Ok(rows
            .iter()
            .map(|r| Subscription {
                endpoint: r.get("endpoint"),
                p256dh: r.get("p256dh"),
                auth: r.get("auth"),
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(uid: u32, flags: &str) -> NewMessage {
        NewMessage {
            uid,
            modseq: uid as u64,
            flags: flags.into(),
            internal_date: 0,
            size: 10,
        }
    }

    async fn folder(db: &Db) -> Folder {
        db.upsert_folder("main", "INBOX", Some("inbox"))
            .await
            .unwrap();
        db.folders("main").await.unwrap().remove(0)
    }

    #[tokio::test]
    async fn folders_are_per_account() {
        let db = Db::memory().await.unwrap();
        db.upsert_folder("main", "INBOX", Some("inbox"))
            .await
            .unwrap();
        db.upsert_folder("catch", "INBOX", Some("inbox"))
            .await
            .unwrap();
        db.upsert_folder("main", "Sent", None).await.unwrap();
        db.upsert_folder("main", "Sent", Some("sent"))
            .await
            .unwrap();
        let main = db.folders("main").await.unwrap();
        assert_eq!(main.len(), 2);
        assert_eq!(main[1].role.as_deref(), Some("sent"));
        assert_eq!(main[1].uidnext, 1);
        assert_eq!(db.folders("catch").await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn insert_is_idempotent_and_updates_flags() {
        let db = Db::memory().await.unwrap();
        let f = folder(&db).await;
        db.insert_messages(f.id, &[msg(1, ""), msg(2, "\\Seen")])
            .await
            .unwrap();
        db.insert_messages(f.id, &[msg(2, "\\Seen \\Flagged")])
            .await
            .unwrap();
        assert_eq!(db.count(f.id).await.unwrap(), 2);
        db.update_flags(f.id, &[(1, 9, "\\Answered".into())])
            .await
            .unwrap();
        let flags: String = sqlx::query_scalar("SELECT flags FROM message WHERE uid = 1")
            .fetch_one(&db.pool)
            .await
            .unwrap();
        assert_eq!(flags, "\\Answered");
    }

    #[tokio::test]
    async fn reset_drops_messages_and_state() {
        let db = Db::memory().await.unwrap();
        let f = folder(&db).await;
        db.insert_messages(f.id, &[msg(1, "")]).await.unwrap();
        db.set_folder_state(f.id, 5, 100).await.unwrap();
        db.reset_folder(f.id, 42).await.unwrap();
        let f = db.folders("main").await.unwrap().remove(0);
        assert_eq!(f.uidvalidity, Some(42));
        assert_eq!(f.uidnext, 1);
        assert_eq!(f.highestmodseq, 0);
        assert_eq!(db.count(f.id).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn blobs_track_references() {
        let db = Db::memory().await.unwrap();
        let f = folder(&db).await;
        db.insert_messages(f.id, &[msg(1, ""), msg(2, ""), msg(3, "")])
            .await
            .unwrap();
        assert_eq!(db.missing_blobs(f.id, 2).await.unwrap(), vec![3, 2]);
        db.set_blob(f.id, 3, "aa").await.unwrap();
        db.set_blob(f.id, 2, "aa").await.unwrap();
        assert_eq!(db.missing_blobs(f.id, 10).await.unwrap(), vec![1]);
        assert_eq!(
            db.next_missing("main", 10).await.unwrap(),
            Some((f.id, vec![1]))
        );
        assert_eq!(db.next_missing("catch", 10).await.unwrap(), None);
        assert_eq!(
            db.referenced_blobs().await.unwrap(),
            HashSet::from(["aa".to_string()])
        );
        db.remove_messages(f.id, &[2, 3]).await.unwrap();
        assert!(db.referenced_blobs().await.unwrap().is_empty());
        assert_eq!(db.uids(f.id).await.unwrap(), vec![1]);
    }

    #[tokio::test]
    async fn dropping_a_folder_drops_its_messages() {
        let db = Db::memory().await.unwrap();
        let f = folder(&db).await;
        db.insert_messages(f.id, &[msg(1, "")]).await.unwrap();
        db.drop_folder(f.id).await.unwrap();
        assert!(db.folders("main").await.unwrap().is_empty());
        let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM message")
            .fetch_one(&db.pool)
            .await
            .unwrap();
        assert_eq!(n, 0);
    }

    fn parsed(id: &str, parents: &[&str], subject: &str) -> Parsed {
        Parsed {
            message_id: id.into(),
            parents: parents.iter().map(|p| p.to_string()).collect(),
            subject: subject.into(),
            body: format!("body of {subject}"),
            ..Default::default()
        }
    }

    async fn search(db: &Db, q: &str) -> Vec<String> {
        db.list(None, &crate::query::Query::parse(q), None, 10)
            .await
            .unwrap()
            .into_iter()
            .map(|m| m.subject)
            .collect()
    }

    async fn thread_of(db: &Db, blob: &str) -> i64 {
        sqlx::query_scalar("SELECT thread FROM parsed WHERE blob = ?")
            .bind(blob)
            .fetch_one(&db.pool)
            .await
            .unwrap()
    }

    async fn with_blobs(db: &Db, blobs: &[&str]) {
        let f = folder(db).await;
        let msgs: Vec<NewMessage> = (1..=blobs.len() as u32).map(|u| msg(u, "")).collect();
        db.insert_messages(f.id, &msgs).await.unwrap();
        for (i, b) in blobs.iter().enumerate() {
            db.set_blob(f.id, i as u32 + 1, b).await.unwrap();
        }
    }

    #[tokio::test]
    async fn threads_join_through_references_in_any_order() {
        let db = Db::memory().await.unwrap();
        with_blobs(&db, &["c", "a", "x", "b"]).await;
        db.store_parsed(
            1,
            &[("c".into(), parsed("3@x", &["1@x", "2@x"], "Re: plan"))],
        )
        .await
        .unwrap();
        db.store_parsed(1, &[("a".into(), parsed("1@x", &[], "plan"))])
            .await
            .unwrap();
        db.store_parsed(1, &[("x".into(), parsed("9@y", &[], "other"))])
            .await
            .unwrap();
        assert_eq!(thread_of(&db, "a").await, thread_of(&db, "c").await);
        assert_ne!(thread_of(&db, "a").await, thread_of(&db, "x").await);

        db.store_parsed(1, &[("b".into(), parsed("4@y", &["9@y", "3@x"], "merge"))])
            .await
            .unwrap();
        let t = thread_of(&db, "a").await;
        for blob in ["b", "c", "x"] {
            assert_eq!(thread_of(&db, blob).await, t);
        }
    }

    #[tokio::test]
    async fn search_finds_words_and_prefixes_and_survives_reparse() {
        let db = Db::memory().await.unwrap();
        with_blobs(&db, &["a", "b"]).await;
        let mut invoice = parsed("1@x", &[], "Your invoice");
        invoice.from = Some(Mailbox {
            name: Some("Billing Team".into()),
            addr: "billing@shop.example.net".into(),
        });
        db.store_parsed(
            1,
            &[
                ("a".into(), invoice.clone()),
                ("b".into(), parsed("2@x", &[], "Café night")),
            ],
        )
        .await
        .unwrap();
        assert_eq!(search(&db, "invoice").await, vec!["Your invoice"]);
        assert_eq!(search(&db, "billing tea").await, vec!["Your invoice"]);
        assert_eq!(search(&db, "cafe").await, vec!["Café night"]);
        assert_eq!(search(&db, "\"quoted\" OR").await, Vec::<String>::new());
        assert!(search(&db, "   ").await.len() == 2);

        invoice.subject = "Your receipt".into();
        invoice.body = "paid".into();
        db.store_parsed(2, &[("a".into(), invoice)]).await.unwrap();
        assert!(search(&db, "invoice").await.is_empty());
        assert_eq!(search(&db, "receipt").await, vec!["Your receipt"]);
        assert_eq!(db.unparsed(2, 10).await.unwrap(), vec!["b"]);
    }

    #[tokio::test]
    async fn pruning_drops_parsed_rows_of_removed_messages() {
        let db = Db::memory().await.unwrap();
        with_blobs(&db, &["a", "b"]).await;
        db.store_parsed(
            1,
            &[
                ("a".into(), parsed("1@x", &[], "keep")),
                ("b".into(), parsed("2@x", &[], "gone")),
            ],
        )
        .await
        .unwrap();
        let f = db.folders("main").await.unwrap().remove(0);
        db.remove_messages(f.id, &[2]).await.unwrap();
        assert_eq!(db.prune_parsed().await.unwrap(), 1);
        assert!(search(&db, "gone").await.is_empty());
        assert_eq!(search(&db, "keep").await, vec!["keep"]);
    }

    #[tokio::test]
    async fn moved_messages_keep_their_blob() {
        let db = Db::memory().await.unwrap();
        db.upsert_folder("main", "INBOX", Some("inbox"))
            .await
            .unwrap();
        db.upsert_folder("main", "Archive", Some("archive"))
            .await
            .unwrap();
        let folders = db.folders("main").await.unwrap();
        let (archive, inbox) = (folders[0].id, folders[1].id);
        let at = |uid: u32, date: i64, size: u32| NewMessage {
            uid,
            modseq: 1,
            flags: String::new(),
            internal_date: date,
            size,
        };
        db.insert_messages(inbox, &[at(1, 100, 10), at(2, 200, 20)])
            .await
            .unwrap();
        db.set_blob(inbox, 1, "one").await.unwrap();
        db.set_blob(inbox, 2, "two").await.unwrap();
        let moved = db.moved(inbox, &[1, 2, 3]).await.unwrap();
        assert_eq!(moved.len(), 2);

        db.insert_messages(archive, &[at(7, 100, 10), at(8, 200, 20), at(9, 200, 20)])
            .await
            .unwrap();
        assert_eq!(db.adopt_blobs(archive, &moved).await.unwrap(), 1);
        assert_eq!(
            db.without_blob(archive, &[7, 8, 9]).await.unwrap(),
            vec![8, 9]
        );
    }

    #[tokio::test]
    async fn flags_are_edited_and_messages_located() {
        let db = Db::memory().await.unwrap();
        let f = folder(&db).await;
        db.insert_messages(f.id, &[msg(1, "$Forwarded"), msg(2, "\\Seen")])
            .await
            .unwrap();
        db.edit_flags(f.id, &[1, 2], &["\\Seen", "\\Flagged"], &[])
            .await
            .unwrap();
        db.edit_flags(f.id, &[2], &[], &["\\Seen"]).await.unwrap();
        let flags: Vec<String> = sqlx::query_scalar("SELECT flags FROM message ORDER BY uid")
            .fetch_all(&db.pool)
            .await
            .unwrap();
        assert_eq!(flags, vec!["$Forwarded \\Flagged \\Seen", "\\Flagged"]);
        let ids: Vec<i64> = sqlx::query_scalar("SELECT id FROM message ORDER BY uid")
            .fetch_all(&db.pool)
            .await
            .unwrap();
        let located = db.locate(&[ids[1], 999]).await.unwrap();
        assert_eq!(
            located,
            vec![Located {
                account: "main".into(),
                folder_id: f.id,
                uid: 2
            }]
        );
    }

    #[tokio::test]
    async fn message_ids_are_never_reused() {
        let db = Db::memory().await.unwrap();
        let f = folder(&db).await;
        db.insert_messages(f.id, &[msg(1, ""), msg(2, "")])
            .await
            .unwrap();
        let ids = |db: Db| async move {
            sqlx::query_scalar::<_, i64>("SELECT id FROM message ORDER BY id")
                .fetch_all(&db.pool)
                .await
                .unwrap()
        };
        let before = ids(db.clone()).await;
        db.remove_messages(f.id, &[2]).await.unwrap();
        db.insert_messages(f.id, &[msg(3, "")]).await.unwrap();
        let after = ids(db.clone()).await;
        assert_eq!(after[0], before[0]);
        assert!(after[1] > before[1]);
    }

    #[tokio::test]
    async fn fresh_unread_inbox_mail_is_notified_once() {
        let db = Db::memory().await.unwrap();
        db.upsert_folder("main", "INBOX", Some("inbox"))
            .await
            .unwrap();
        db.upsert_folder("main", "Archive", Some("archive"))
            .await
            .unwrap();
        let folders = db.folders("main").await.unwrap();
        let (archive, inbox) = (folders[0].id, folders[1].id);
        let at = |uid: u32, date: i64, flags: &str| NewMessage {
            uid,
            modseq: 1,
            flags: flags.into(),
            internal_date: date,
            size: 1,
        };
        db.insert_messages(
            inbox,
            &[at(1, 1000, ""), at(2, 10, ""), at(3, 1000, "\\Seen")],
        )
        .await
        .unwrap();
        db.insert_messages(archive, &[at(1, 1000, "")])
            .await
            .unwrap();
        for (folder, uid, blob) in [
            (inbox, 1, "new"),
            (inbox, 2, "old"),
            (inbox, 3, "read"),
            (archive, 1, "filed"),
        ] {
            db.set_blob(folder, uid, blob).await.unwrap();
        }
        let items: Vec<(String, Parsed)> = ["new", "old", "read", "filed"]
            .into_iter()
            .map(|b| (b.to_string(), parsed(&format!("{b}@x"), &[], b)))
            .collect();
        db.store_parsed(1, &items).await.unwrap();
        let blobs: Vec<String> = ["new", "old", "read", "filed"].map(String::from).to_vec();
        let fresh = db.fresh_inbox(&blobs, 500).await.unwrap();
        assert_eq!(
            fresh.iter().map(|f| f.subject.as_str()).collect::<Vec<_>>(),
            vec!["new"]
        );
        assert!(db.mark_notified("new@x", 1).await.unwrap());
        assert!(!db.mark_notified("new@x", 2).await.unwrap());
        assert_eq!(db.prune_notified(5).await.unwrap(), 1);
    }

    #[tokio::test]
    async fn subscriptions_upsert_and_remove() {
        let db = Db::memory().await.unwrap();
        let sub = |auth: &str| Subscription {
            endpoint: "https://push.example.com/up/1".into(),
            p256dh: "key".into(),
            auth: auth.into(),
        };
        db.add_subscription(&sub("a"), 1).await.unwrap();
        db.add_subscription(&sub("b"), 2).await.unwrap();
        assert_eq!(db.subscriptions().await.unwrap(), vec![sub("b")]);
        assert!(
            db.remove_subscription("https://push.example.com/up/1")
                .await
                .unwrap()
        );
        assert!(db.subscriptions().await.unwrap().is_empty());
    }
}
