use std::collections::{BTreeSet, HashSet};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use async_imap::types::{Fetch, Name};
use futures_util::TryStreamExt;
use tokio::sync::Notify;
use tokio::sync::mpsc::{self, UnboundedReceiver};

use crate::config::AccountConfig;
use crate::db::{Db, Folder, NewMessage};
use crate::hub::{Hub, folder_key};
use crate::imap::{self, Session};
use crate::store::Blobs;
use crate::watch::{self, Event};

// imap reads have no timeout of their own, a silently dead connection would hang forever
const STALL: Duration = Duration::from_secs(5 * 60);
const FULL_SWEEP: Duration = Duration::from_secs(15 * 60);
const MAX_BACKOFF: Duration = Duration::from_secs(300);
const BODY_BATCH: u32 = 50;
// a bigger arrival is left to the backfill so it can't hold up other folders
const LIVE_BODIES: usize = 200;

pub async fn run(
    cfg: AccountConfig,
    db: Db,
    blobs: Arc<Blobs>,
    hub: Arc<Hub>,
    indexer: Arc<Notify>,
) {
    let (tx, mut rx) = mpsc::unbounded_channel();
    tokio::spawn(watch::run(cfg.clone(), tx));
    let mut backoff = Duration::from_secs(5);
    loop {
        let started = tokio::time::Instant::now();
        if let Err(e) = session(&cfg, &db, &blobs, &hub, &indexer, &mut rx).await {
            tracing::warn!(account = cfg.name, "sync failed: {e:#}");
        }
        if started.elapsed() > MAX_BACKOFF {
            backoff = Duration::from_secs(5);
        }
        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(MAX_BACKOFF);
    }
}

async fn session(
    cfg: &AccountConfig,
    db: &Db,
    blobs: &Blobs,
    hub: &Hub,
    indexer: &Notify,
    rx: &mut UnboundedReceiver<Event>,
) -> Result<()> {
    let mut sync = Sync {
        account: &cfg.name,
        db,
        blobs,
        hub,
        indexer,
        imap: imap::connect(cfg).await?,
        backfilled: 0,
    };
    bounded(sync.all()).await?;
    let mut sweep = tokio::time::interval(FULL_SWEEP);
    sweep.tick().await;
    let mut backfilling = true;
    loop {
        tokio::select! {
            biased;
            ev = rx.recv() => {
                backfilling = true;
                let mut events = vec![ev.context("watcher has stopped")?];
                while let Ok(ev) = rx.try_recv() {
                    events.push(ev);
                }
                if events.contains(&Event::Folders) {
                    bounded(sync.all()).await?;
                } else {
                    let paths: BTreeSet<String> = events
                        .into_iter()
                        .filter_map(|e| match e {
                            Event::Folder(p) => Some(p),
                            Event::Folders => None,
                        })
                        .collect();
                    for path in paths {
                        bounded(sync.one(&path)).await?;
                    }
                }
            }
            _ = sweep.tick() => {
                backfilling = true;
                bounded(sync.all()).await?;
            }
            _ = std::future::ready(()), if backfilling => {
                backfilling = bounded(sync.backfill()).await?;
            }
        }
    }
}

async fn bounded<T>(fut: impl std::future::Future<Output = Result<T>>) -> Result<T> {
    tokio::time::timeout(STALL, fut)
        .await
        .map_err(anyhow::Error::from)
        .context("the mail server stopped answering")?
}

struct Sync<'a> {
    account: &'a str,
    db: &'a Db,
    blobs: &'a Blobs,
    hub: &'a Hub,
    indexer: &'a Notify,
    imap: Session,
    backfilled: usize,
}

impl Sync<'_> {
    async fn all(&mut self) -> Result<()> {
        let names: Vec<Name> = self
            .imap
            .list(Some(""), Some("*"))
            .await?
            .try_collect()
            .await?;
        let mut live = HashSet::new();
        for name in names.iter().filter(|n| imap::selectable(n)) {
            self.db
                .upsert_folder(self.account, name.name(), imap::role(name))
                .await?;
            live.insert(name.name().to_string());
        }
        let mut folders = self.db.folders(self.account).await?;
        for f in folders.iter().filter(|f| !live.contains(&f.path)) {
            tracing::info!(account = self.account, folder = f.path, "folder is gone");
            self.db.drop_folder(f.id).await?;
        }
        folders.retain(|f| live.contains(&f.path));
        folders.sort_by_key(|f| f.role.as_deref() != Some("inbox"));
        for f in &folders {
            self.folder(f).await?;
        }
        self.drain();
        Ok(())
    }

    async fn one(&mut self, path: &str) -> Result<()> {
        let folders = self.db.folders(self.account).await?;
        match folders.into_iter().find(|f| f.path == path) {
            Some(f) => self.folder(&f).await?,
            None => self.all().await?,
        }
        self.drain();
        Ok(())
    }

    // only notifications from the watcher connection matter, these would just pile up
    fn drain(&mut self) {
        while self.imap.unsolicited_responses.try_recv().is_ok() {}
    }

    async fn folder(&mut self, known: &Folder) -> Result<()> {
        let mut f = known.clone();
        let mb = self.imap.select_condstore(&f.path).await?;
        let uidvalidity = mb.uid_validity.context("server sent no UIDVALIDITY")?;
        if f.uidvalidity != Some(uidvalidity) {
            if f.uidvalidity.is_some() {
                tracing::warn!(
                    account = self.account,
                    folder = f.path,
                    "UIDVALIDITY changed, refetching"
                );
            }
            self.db.reset_folder(f.id, uidvalidity).await?;
            f.uidvalidity = Some(uidvalidity);
            f.uidnext = 1;
            f.highestmodseq = 0;
        }
        let Some(highest) = mb.highest_modseq else {
            bail!("{} has no HIGHESTMODSEQ, CONDSTORE is required", f.path);
        };
        let uid_next = mb.uid_next.unwrap_or(1);

        let mut changed = 0;
        if f.uidnext > 1 && f.highestmodseq > 0 && highest > f.highestmodseq {
            let fetches: Vec<Fetch> = self
                .imap
                .uid_fetch(
                    format!("1:{}", f.uidnext - 1),
                    format!("(UID FLAGS) (CHANGEDSINCE {})", f.highestmodseq),
                )
                .await?
                .try_collect()
                .await?;
            let changes: Vec<(u32, u64, String)> = fetches
                .iter()
                .filter_map(|m| Some((m.uid?, m.modseq.unwrap_or(highest), imap::flags(m.flags()))))
                .collect();
            changed = changes.len();
            self.db.update_flags(f.id, &changes).await?;
        }

        let mut next = f.uidnext.max(1);
        let mut new_uids = Vec::new();
        if mb.exists > 0 && uid_next > f.uidnext {
            let fetches: Vec<Fetch> = self
                .imap
                .uid_fetch(
                    format!("{}:*", f.uidnext.max(1)),
                    "(UID FLAGS MODSEQ INTERNALDATE RFC822.SIZE)",
                )
                .await?
                .try_collect()
                .await?;
            let new: Vec<NewMessage> = fetches
                .iter()
                .filter_map(|m| {
                    let uid = m.uid.filter(|u| *u >= f.uidnext)?;
                    Some(NewMessage {
                        uid,
                        modseq: m.modseq.unwrap_or(highest),
                        flags: imap::flags(m.flags()),
                        internal_date: m.internal_date().map_or(0, |d| d.timestamp()),
                        size: m.size.unwrap_or(0),
                    })
                })
                .collect();
            new_uids = new.iter().map(|m| m.uid).collect();
            if let Some(max) = new.iter().map(|m| m.uid).max() {
                next = max + 1;
            }
            self.db.insert_messages(f.id, &new).await?;
        }
        next = next.max(uid_next);

        let mut removed = 0;
        if self.db.count(f.id).await? != mb.exists {
            let live = self.imap.uid_search("ALL").await?;
            let gone: Vec<u32> = self
                .db
                .uids(f.id)
                .await?
                .into_iter()
                .filter(|u| !live.contains(u))
                .collect();
            removed = gone.len();
            self.db.remove_messages(f.id, &gone).await?;
        }

        self.db.set_folder_state(f.id, next, highest).await?;
        let added = new_uids.len();
        if changed + removed > 0 {
            self.hub.changed([folder_key(f.role.as_deref(), &f.path)]);
        }
        if added + changed + removed > 0 {
            tracing::info!(
                account = self.account,
                folder = f.path,
                added,
                changed,
                removed,
                "synced"
            );
        }
        if added <= LIVE_BODIES {
            self.fetch_bodies(&f, &new_uids).await?;
        }
        Ok(())
    }

    async fn backfill(&mut self) -> Result<bool> {
        let Some((folder_id, uids)) = self.db.next_missing(self.account, BODY_BATCH).await? else {
            if self.backfilled > 0 {
                tracing::info!(
                    account = self.account,
                    downloaded = self.backfilled,
                    "all messages downloaded"
                );
                self.backfilled = 0;
            }
            return Ok(false);
        };
        let folders = self.db.folders(self.account).await?;
        let f = folders
            .into_iter()
            .find(|f| f.id == folder_id)
            .context("folder vanished during backfill")?;
        self.imap.select_condstore(&f.path).await?;
        let got = self.fetch_bodies(&f, &uids).await?;
        if got == 0 {
            // the uids were expunged since the last sync, resyncing drops their rows
            self.folder(&f).await?;
            return Ok(false);
        }
        let before = self.backfilled;
        self.backfilled += got;
        if self.backfilled / 500 > before / 500 {
            tracing::info!(
                account = self.account,
                downloaded = self.backfilled,
                "downloading older messages"
            );
        }
        Ok(true)
    }

    async fn fetch_bodies(&mut self, f: &Folder, uids: &[u32]) -> Result<usize> {
        let mut uids = uids.to_vec();
        uids.sort_unstable_by(|a, b| b.cmp(a));
        let mut got = 0;
        for batch in uids.chunks(BODY_BATCH as usize) {
            let fetches: Vec<Fetch> = self
                .imap
                .uid_fetch(imap::uid_set(batch), "(UID BODY.PEEK[])")
                .await?
                .try_collect()
                .await?;
            for m in &fetches {
                if let (Some(uid), Some(body)) = (m.uid, m.body()) {
                    let id = self.blobs.put(body)?;
                    self.db.set_blob(f.id, uid, &id).await?;
                    got += 1;
                }
            }
        }
        if got > 0 {
            self.indexer.notify_one();
        }
        Ok(got)
    }
}
