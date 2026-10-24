use std::collections::{BTreeMap, HashSet};

use anyhow::Result;
use serde::Serialize;
use sqlx::sqlite::SqliteRow;
use sqlx::{QueryBuilder, Row, Sqlite};

use crate::db::Db;
use crate::hub::folder_key;
use crate::parse::{Attachment, Mailbox};
use crate::query::Query;

const ROLES: &[(&str, &str)] = &[
    ("inbox", "Inbox"),
    ("archive", "Archive"),
    ("sent", "Sent"),
    ("drafts", "Drafts"),
    ("junk", "Junk"),
    ("trash", "Trash"),
];

#[derive(Debug, Serialize, PartialEq)]
pub struct FolderView {
    pub key: String,
    pub name: String,
    pub total: i64,
    pub unread: i64,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct MessageRow {
    pub id: i64,
    pub thread: i64,
    pub date: i64,
    pub folder: String,
    pub from: Option<Mailbox>,
    pub subject: String,
    pub snippet: String,
    pub recipient: Option<String>,
    pub unread: bool,
    pub flagged: bool,
    pub answered: bool,
    pub attachments: bool,
}

#[derive(Debug, Serialize)]
pub struct MessageMeta {
    #[serde(flatten)]
    pub row: MessageRow,
    pub message_id: String,
    pub to: Vec<Mailbox>,
    pub cc: Vec<Mailbox>,
    pub reply_to: Vec<Mailbox>,
    pub attachment_list: Vec<Attachment>,
    #[serde(skip)]
    pub blob: String,
    #[serde(skip)]
    pub account: String,
    #[serde(skip)]
    pub parents: Vec<String>,
    #[serde(skip)]
    pub recipient_hosted: bool,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct Conversation {
    #[serde(flatten)]
    pub latest: MessageRow,
    pub messages: i64,
}

pub struct Cursor {
    pub date: i64,
    pub id: i64,
}

macro_rules! with_row {
    ($select_tail:literal, $rest:literal) => {
        concat!(
            "SELECT m.id AS id, p.thread, COALESCE(p.date, m.internal_date) AS date, f.role, f.path, ",
            "p.from_name, p.from_addr, p.subject, p.snippet, p.recipient, m.flags, p.attachments_json",
            $select_tail,
            " FROM message m JOIN folder f ON f.id = m.folder_id JOIN parsed p ON p.blob = m.blob ",
            $rest
        )
    };
}

fn row(r: &SqliteRow) -> MessageRow {
    let flags: String = r.get("flags");
    let has = |f: &str| flags.split(' ').any(|x| x == f);
    let from_addr: Option<String> = r.get("from_addr");
    let attachments: String = r.get("attachments_json");
    MessageRow {
        id: r.get("id"),
        thread: r.get("thread"),
        date: r.get("date"),
        folder: folder_key(r.get::<Option<&str>, _>("role"), r.get("path")),
        from: from_addr.map(|addr| Mailbox {
            name: r.get("from_name"),
            addr,
        }),
        subject: r.get("subject"),
        snippet: r.get("snippet"),
        recipient: r.get("recipient"),
        unread: !has("\\Seen"),
        flagged: has("\\Flagged"),
        answered: has("\\Answered"),
        attachments: attachments != "[]",
    }
}

pub fn is_role(key: &str) -> bool {
    ROLES.iter().any(|(r, _)| *r == key)
}

fn push_filters(qb: &mut QueryBuilder<Sqlite>, folder: Option<&str>, query: &Query) {
    if let Some(key) = query.folder.as_deref().or(folder) {
        folder_filter(qb, key);
    }
    if let Some(fts) = query.fts() {
        qb.push(" AND p.id IN (SELECT rowid FROM search WHERE search MATCH ")
            .push_bind(fts)
            .push(")");
    }
    match query.unread {
        Some(true) => {
            qb.push(" AND (' ' || m.flags || ' ') NOT LIKE '% \\Seen %'");
        }
        Some(false) => {
            qb.push(" AND (' ' || m.flags || ' ') LIKE '% \\Seen %'");
        }
        None => {}
    }
    if query.flagged {
        qb.push(" AND (' ' || m.flags || ' ') LIKE '% \\Flagged %'");
    }
    if query.attachment {
        qb.push(" AND p.attachments_json != '[]'");
    }
    if let Some(t) = query.before {
        qb.push(" AND COALESCE(p.date, m.internal_date) < ")
            .push_bind(t);
    }
    if let Some(t) = query.after {
        qb.push(" AND COALESCE(p.date, m.internal_date) >= ")
            .push_bind(t);
    }
}

fn folder_filter(qb: &mut QueryBuilder<Sqlite>, key: &str) {
    if is_role(key) {
        qb.push(" AND f.role = ").push_bind(key.to_string());
    } else {
        qb.push(" AND f.role IS NULL AND f.path = ")
            .push_bind(key.to_string());
    }
}

impl Db {
    pub async fn folder_views(&self) -> Result<Vec<FolderView>> {
        let rows = sqlx::query(
            "SELECT f.role, f.path, COUNT(m.uid) AS total,
                SUM(m.uid IS NOT NULL AND (' ' || m.flags || ' ') NOT LIKE '% \\Seen %') AS unread
             FROM folder f LEFT JOIN message m ON m.folder_id = f.id
             GROUP BY f.id",
        )
        .fetch_all(&self.pool)
        .await?;
        let mut merged: BTreeMap<String, (i64, i64)> = BTreeMap::new();
        for r in &rows {
            let role: Option<&str> = r.get("role");
            if matches!(role, Some("all" | "flagged")) {
                continue;
            }
            let entry = merged.entry(folder_key(role, r.get("path"))).or_default();
            entry.0 += r.get::<i64, _>("total");
            entry.1 += r.get::<Option<i64>, _>("unread").unwrap_or(0);
        }
        let mut out = Vec::new();
        for (role, name) in ROLES {
            if let Some((total, unread)) = merged.remove(*role) {
                out.push(FolderView {
                    key: role.to_string(),
                    name: name.to_string(),
                    total,
                    unread,
                });
            }
        }
        out.extend(merged.into_iter().map(|(key, (total, unread))| FolderView {
            name: key.clone(),
            key,
            total,
            unread,
        }));
        Ok(out)
    }

    pub async fn list(
        &self,
        folder: Option<&str>,
        query: &Query,
        before: Option<Cursor>,
        limit: u32,
    ) -> Result<Vec<MessageRow>> {
        let mut qb = QueryBuilder::<Sqlite>::new(with_row!("", "WHERE 1 = 1"));
        push_filters(&mut qb, folder, query);
        if let Some(c) = before {
            qb.push(" AND (COALESCE(p.date, m.internal_date), m.id) < (")
                .push_bind(c.date)
                .push(", ")
                .push_bind(c.id)
                .push(")");
        }
        qb.push(" ORDER BY date DESC, id DESC LIMIT ")
            .push_bind(limit as i64);
        let rows = qb.build().fetch_all(&self.pool).await?;
        Ok(rows.iter().map(row).collect())
    }

    pub async fn conversations(
        &self,
        folder: Option<&str>,
        query: &Query,
        before: Option<Cursor>,
        limit: u32,
    ) -> Result<Vec<Conversation>> {
        let mut qb = QueryBuilder::<Sqlite>::new("WITH base AS (");
        qb.push(with_row!(
            ", ((' ' || m.flags || ' ') NOT LIKE '% \\Seen %') AS is_unread",
            "WHERE 1 = 1"
        ));
        push_filters(&mut qb, folder, query);
        qb.push(
            "), ranked AS (SELECT *, ROW_NUMBER() OVER (PARTITION BY thread ORDER BY date DESC, id DESC) AS rn, \
             COUNT(*) OVER (PARTITION BY thread) AS messages, MAX(is_unread) OVER (PARTITION BY thread) AS any_unread FROM base) \
             SELECT * FROM ranked WHERE rn = 1",
        );
        if let Some(c) = before {
            qb.push(" AND (date, id) < (")
                .push_bind(c.date)
                .push(", ")
                .push_bind(c.id)
                .push(")");
        }
        qb.push(" ORDER BY date DESC, id DESC LIMIT ")
            .push_bind(limit as i64);
        let rows = qb.build().fetch_all(&self.pool).await?;
        Ok(rows
            .iter()
            .map(|r| {
                let mut latest = row(r);
                latest.unread = r.get::<bool, _>("any_unread");
                Conversation {
                    latest,
                    messages: r.get("messages"),
                }
            })
            .collect())
    }

    pub async fn thread(&self, thread: i64) -> Result<Vec<MessageRow>> {
        let rows = sqlx::query(with_row!(
            ", p.message_id",
            "WHERE p.thread = ? ORDER BY date, id"
        ))
        .bind(thread)
        .fetch_all(&self.pool)
        .await?;
        let rank = |folder: &str| match folder {
            "inbox" => 0,
            "archive" => 1,
            "sent" => 3,
            "drafts" => 4,
            "junk" => 5,
            "trash" => 6,
            _ => 2,
        };
        let mut best: BTreeMap<String, MessageRow> = BTreeMap::new();
        for r in &rows {
            let m = row(r);
            let id: String = r.get("message_id");
            match best.get(&id) {
                Some(kept) if rank(&kept.folder) <= rank(&m.folder) => {}
                _ => {
                    best.insert(id, m);
                }
            }
        }
        let mut out: Vec<MessageRow> = best.into_values().collect();
        out.sort_by_key(|m| (m.date, m.id));
        Ok(out)
    }

    pub async fn message(&self, id: i64) -> Result<Option<MessageMeta>> {
        let r = sqlx::query(with_row!(
            ", p.message_id, p.to_json, p.cc_json, p.blob, f.account, p.reply_to_json, p.parents_json, p.recipient_hosted",
            "WHERE m.id = ?"
        ))
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        let Some(r) = r else {
            return Ok(None);
        };
        Ok(Some(MessageMeta {
            row: row(&r),
            message_id: r.get("message_id"),
            to: serde_json::from_str(r.get("to_json"))?,
            cc: serde_json::from_str(r.get("cc_json"))?,
            reply_to: serde_json::from_str(r.get("reply_to_json"))?,
            attachment_list: serde_json::from_str(r.get("attachments_json"))?,
            blob: r.get("blob"),
            account: r.get("account"),
            parents: serde_json::from_str(r.get("parents_json"))?,
            recipient_hosted: r.get("recipient_hosted"),
        }))
    }

    pub async fn own(&self, logins: &[String]) -> Result<crate::compose::Own> {
        let mut own = crate::compose::Own::default();
        for login in logins {
            own.addresses.insert(login.to_ascii_lowercase());
            if let Some(d) = crate::compose::domain(login) {
                own.domains.insert(d.to_ascii_lowercase());
            }
        }
        let rows = sqlx::query(
            "SELECT DISTINCT recipient, recipient_hosted FROM parsed WHERE recipient IS NOT NULL",
        )
        .fetch_all(&self.pool)
        .await?;
        for r in &rows {
            let addr: String = r.get("recipient");
            if r.get::<bool, _>("recipient_hosted")
                && let Some(d) = crate::compose::domain(&addr)
            {
                own.domains.insert(d.to_string());
            }
            own.addresses.insert(addr);
        }
        Ok(own)
    }

    pub async fn sender_account(&self, domain: &str) -> Result<Option<String>> {
        Ok(sqlx::query_scalar(
            "SELECT f.account FROM message m JOIN folder f ON f.id = m.folder_id JOIN parsed p ON p.blob = m.blob
             WHERE p.recipient_hosted = 1 AND substr(p.recipient, instr(p.recipient, '@') + 1) = ?
             ORDER BY COALESCE(p.date, m.internal_date) DESC LIMIT 1",
        )
        .bind(domain)
        .fetch_optional(&self.pool)
        .await?)
    }

    pub async fn display_name(&self, addr: &str) -> Result<Option<String>> {
        Ok(sqlx::query_scalar(
            "SELECT p.from_name FROM message m JOIN folder f ON f.id = m.folder_id JOIN parsed p ON p.blob = m.blob
             WHERE f.role = 'sent' AND p.from_addr = ? AND p.from_name IS NOT NULL
             ORDER BY COALESCE(p.date, m.internal_date) DESC LIMIT 1",
        )
        .bind(addr)
        .fetch_optional(&self.pool)
        .await?)
    }

    pub async fn folders_of_blobs(&self, blobs: &[String]) -> Result<HashSet<String>> {
        let mut qb = QueryBuilder::<Sqlite>::new(
            "SELECT DISTINCT f.role, f.path FROM message m JOIN folder f ON f.id = m.folder_id WHERE m.blob IN (",
        );
        let mut sep = qb.separated(", ");
        for b in blobs {
            sep.push_bind(b.clone());
        }
        qb.push(")");
        let rows = qb.build().fetch_all(&self.pool).await?;
        Ok(rows
            .iter()
            .map(|r| folder_key(r.get::<Option<&str>, _>("role"), r.get("path")))
            .collect())
    }
}

#[derive(Debug, Serialize, PartialEq)]
pub struct AddressInfo {
    pub address: String,
    pub count: i64,
    pub mailbox: bool,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct AddressGroup {
    pub domain: String,
    pub catch_all: bool,
    pub addresses: Vec<AddressInfo>,
}

impl Db {
    pub async fn sendable_addresses(
        &self,
        logins: &[String],
        catch_all: &[String],
    ) -> Result<Vec<AddressGroup>> {
        let rows = sqlx::query(
            "SELECT recipient, COUNT(*) AS n FROM parsed WHERE recipient_hosted = 1 GROUP BY recipient",
        )
        .fetch_all(&self.pool)
        .await?;
        let mut counts: BTreeMap<String, i64> = rows
            .iter()
            .map(|r| (r.get::<String, _>("recipient"), r.get::<i64, _>("n")))
            .collect();
        let logins: Vec<String> = logins.iter().map(|l| l.to_ascii_lowercase()).collect();
        for login in &logins {
            counts.entry(login.clone()).or_insert(0);
        }
        let mut groups: BTreeMap<String, Vec<AddressInfo>> = BTreeMap::new();
        for (address, count) in counts {
            let Some(domain) = crate::compose::domain(&address).map(str::to_string) else {
                continue;
            };
            groups.entry(domain).or_default().push(AddressInfo {
                mailbox: logins.contains(&address),
                address,
                count,
            });
        }
        let mut out: Vec<AddressGroup> = groups
            .into_iter()
            .map(|(domain, mut addresses)| {
                addresses.sort_by(|a, b| b.count.cmp(&a.count).then(a.address.cmp(&b.address)));
                AddressGroup {
                    catch_all: catch_all.contains(&domain),
                    domain,
                    addresses,
                }
            })
            .collect();
        out.sort_by_key(|g| -g.addresses.iter().map(|a| a.count).sum::<i64>());
        Ok(out)
    }

    pub async fn suggested_from(
        &self,
        to: &str,
        own: &crate::compose::Own,
    ) -> Result<Option<String>> {
        let to = to.trim().to_ascii_lowercase();
        let received: Option<(String, i64)> = sqlx::query(
            "SELECT recipient, COALESCE(date, 0) AS d FROM parsed
             WHERE from_addr = ? AND recipient_hosted = 1 ORDER BY d DESC LIMIT 1",
        )
        .bind(&to)
        .fetch_optional(&self.pool)
        .await?
        .map(|r| (r.get("recipient"), r.get("d")));
        let pattern = format!("%\"addr\":\"{}\"%", to.replace(['%', '_'], ""));
        let sent_rows = sqlx::query(
            "SELECT from_addr, COALESCE(date, 0) AS d FROM parsed
             WHERE from_addr IS NOT NULL AND (to_json LIKE ? OR cc_json LIKE ?) ORDER BY d DESC LIMIT 20",
        )
        .bind(&pattern)
        .bind(&pattern)
        .fetch_all(&self.pool)
        .await?;
        let sent: Option<(String, i64)> = sent_rows
            .iter()
            .map(|r| (r.get::<String, _>("from_addr"), r.get::<i64, _>("d")))
            .find(|(addr, _)| own.contains(addr));
        Ok(match (received, sent) {
            (Some(r), Some(s)) => Some(if s.1 >= r.1 { s.0 } else { r.0 }),
            (Some(r), None) => Some(r.0),
            (None, Some(s)) => Some(s.0),
            (None, None) => None,
        })
    }
}
