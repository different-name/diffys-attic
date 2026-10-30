use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::Result;

use crate::db::Db;
use crate::hub::Hub;
use crate::images::Images;
use crate::push::{Delivery, Notice, Pusher};
use crate::store::Blobs;

// older mail reaching the inbox is a move or a first sync, not something new
const FRESH: Duration = Duration::from_secs(15 * 60);

pub struct Notifier {
    pub db: Db,
    pub hub: Arc<Hub>,
    pub pusher: Arc<Pusher>,
    pub images: Arc<Images>,
    pub blobs: Arc<Blobs>,
}

pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

impl Notifier {
    // fetching images on arrival means a tracking pixel learns when mail was delivered, not opened
    pub async fn prefetch_images(&self, blobs: &[String]) -> Result<()> {
        let since = now() - FRESH.as_secs() as i64;
        for blob in self.db.fresh_blobs(blobs, since).await? {
            let raw = self.blobs.get(&blob)?;
            let urls = tokio::task::spawn_blocking(move || {
                mail_parser::MessageParser::default()
                    .parse(&raw)
                    .and_then(|m| m.body_html(0).map(|h| crate::clean::clean_html(&h).images))
                    .unwrap_or_default()
            })
            .await?;
            if !urls.is_empty() {
                self.images.prefetch(&urls).await?;
            }
        }
        Ok(())
    }

    pub async fn new_mail(&self, blobs: &[String]) -> Result<()> {
        let since = now() - FRESH.as_secs() as i64;
        for fresh in self.db.fresh_inbox(blobs, since).await? {
            if !self.db.mark_notified(&fresh.message_id, now()).await? {
                continue;
            }
            let notice = Notice::New {
                id: fresh.id,
                thread: fresh.thread,
                from: fresh.from,
                subject: fresh.subject,
                recipient: fresh.recipient,
            };
            self.hub.notice(notice.clone());
            let payload = notice.payload();
            for sub in self.db.subscriptions().await? {
                match self.pusher.send(&sub, &payload).await {
                    Ok(Delivery::Sent) => {}
                    Ok(Delivery::Gone) => {
                        tracing::info!("push subscription expired, removing it");
                        self.db.remove_subscription(&sub.endpoint).await?;
                    }
                    Err(e) => tracing::warn!("push failed: {e:#}"),
                }
            }
        }
        Ok(())
    }
}
