use std::collections::HashSet;
use std::path::Path;
use std::str::FromStr;

use anyhow::{Context, Result};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use sqlx::{Row, SqlitePool};

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

#[derive(Clone)]
pub struct Db {
    pool: SqlitePool,
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
}
