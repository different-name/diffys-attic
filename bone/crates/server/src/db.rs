use std::collections::HashSet;
use std::path::Path;
use std::str::FromStr;

use anyhow::{Context, Result};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use sqlx::{Row, SqlitePool};

use crate::parse::{Mailbox, Parsed};

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
                "INSERT INTO parsed (blob, version, message_id, subject, from_name, from_addr, to_json, cc_json, date, recipient, attachments_json, snippet)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                 ON CONFLICT (blob) DO UPDATE SET version = excluded.version, message_id = excluded.message_id,
                    subject = excluded.subject, from_name = excluded.from_name, from_addr = excluded.from_addr,
                    to_json = excluded.to_json, cc_json = excluded.cc_json, date = excluded.date,
                    recipient = excluded.recipient, attachments_json = excluded.attachments_json, snippet = excluded.snippet
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
}
