use std::time::Duration;

use tokio::time::Instant;

use anyhow::{Context, Result};
use async_imap::extensions::idle::IdleResponse;
use async_imap::imap_proto::{MailboxDatum, Response};
use async_imap::types::UnsolicitedResponse;
use tokio::sync::mpsc::UnboundedSender;

use crate::config::AccountConfig;
use crate::imap::{self, Session};

// migadu's proxy drops a connection 30 min after its last notification, NOOP and re-entering IDLE don't reset that
const RECONNECT_AFTER: Duration = Duration::from_secs(25 * 60);
const MAX_BACKOFF: Duration = Duration::from_secs(300);

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Folder(String),
    Folders,
}

pub async fn run(cfg: AccountConfig, tx: UnboundedSender<Event>) {
    let mut backoff = Duration::from_secs(5);
    loop {
        let started = tokio::time::Instant::now();
        let result = watch(&cfg, &tx).await;
        if tx.is_closed() {
            return;
        }
        if started.elapsed() > MAX_BACKOFF {
            backoff = Duration::from_secs(5);
        }
        if let Err(e) = result {
            tracing::warn!(account = cfg.name, "watcher failed: {e:#}");
            tokio::time::sleep(backoff).await;
            backoff = (backoff * 2).min(MAX_BACKOFF);
        }
        let _ = tx.send(Event::Folders);
    }
}

async fn watch(cfg: &AccountConfig, tx: &UnboundedSender<Event>) -> Result<()> {
    let mut session = imap::connect(cfg).await?;
    let fallback = match session
        .run_command_and_check_ok(
            "NOTIFY SET STATUS (personal (MessageNew MessageExpunge FlagChange MailboxName))",
        )
        .await
    {
        Ok(()) => false,
        Err(e) => {
            tracing::warn!(
                account = cfg.name,
                "NOTIFY refused ({e}), watching INBOX only"
            );
            session.select("INBOX").await?;
            true
        }
    };
    while session.unsolicited_responses.try_recv().is_ok() {}
    tracing::debug!(account = cfg.name, "watching for changes");

    let mut quiet_since = Instant::now();
    loop {
        let remaining = RECONNECT_AFTER.saturating_sub(quiet_since.elapsed());
        if remaining.is_zero() {
            let _ = session.logout().await;
            return Ok(());
        }
        let mut idle = session.idle();
        idle.init().await?;
        let (wait, _stop) = idle.wait_with_timeout(remaining);
        // async-imap restarts its own timeout on every keepalive line, so it never fires on migadu
        let res = match tokio::time::timeout(remaining, wait).await {
            Ok(res) => res?,
            Err(_) => IdleResponse::Timeout,
        };
        session = tokio::time::timeout(Duration::from_secs(30), idle.done())
            .await
            .context("ending IDLE timed out")??;
        let mut notified = false;
        if let IdleResponse::NewData(data) = res {
            notified |= send(tx, event(data.parsed(), fallback))?;
        }
        notified |= drain(&mut session, tx, fallback)?;
        if notified {
            quiet_since = Instant::now();
        }
    }
}

fn drain(session: &mut Session, tx: &UnboundedSender<Event>, fallback: bool) -> Result<bool> {
    let mut sent = false;
    while let Ok(u) = session.unsolicited_responses.try_recv() {
        let ev = match u {
            UnsolicitedResponse::Status { mailbox, .. } => Some(Event::Folder(mailbox)),
            UnsolicitedResponse::Other(data) => event(data.parsed(), fallback),
            UnsolicitedResponse::Exists(_) | UnsolicitedResponse::Expunge(_) if fallback => {
                Some(Event::Folder("INBOX".into()))
            }
            _ => None,
        };
        sent |= send(tx, ev)?;
    }
    Ok(sent)
}

fn send(tx: &UnboundedSender<Event>, ev: Option<Event>) -> Result<bool> {
    let Some(ev) = ev else {
        return Ok(false);
    };
    tx.send(ev).context("sync loop has stopped")?;
    Ok(true)
}

fn event(res: &Response<'_>, fallback: bool) -> Option<Event> {
    match res {
        Response::MailboxData(MailboxDatum::Status { mailbox, .. }) => {
            Some(Event::Folder(mailbox.to_string()))
        }
        Response::MailboxData(MailboxDatum::List { .. }) => Some(Event::Folders),
        Response::MailboxData(MailboxDatum::Exists(_))
        | Response::Expunge(_)
        | Response::Fetch(..)
            if fallback =>
        {
            Some(Event::Folder("INBOX".into()))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(line: &str) -> Response<'_> {
        Response::parse(line.as_bytes()).unwrap().1
    }

    #[test]
    fn status_names_the_folder() {
        let res = parse("* STATUS \"Archive/2026\" (MESSAGES 7 UIDNEXT 9)\r\n");
        assert_eq!(
            event(&res, false),
            Some(Event::Folder("Archive/2026".into()))
        );
    }

    #[test]
    fn list_means_the_folder_tree_changed() {
        let res = parse("* LIST () \"/\" \"Receipts\"\r\n");
        assert_eq!(event(&res, false), Some(Event::Folders));
    }

    #[test]
    fn anything_else_is_ignored_unless_idling_on_inbox() {
        let res = parse("* 3 EXISTS\r\n");
        assert_eq!(event(&res, false), None);
        assert_eq!(event(&res, true), Some(Event::Folder("INBOX".into())));
        let still_here = parse("* OK Still here\r\n");
        assert_eq!(event(&still_here, false), None);
        assert_eq!(event(&still_here, true), None);
    }
}
