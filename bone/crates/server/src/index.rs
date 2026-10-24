use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use tokio::sync::Notify;

use crate::db::Db;
use crate::hub::Hub;
use crate::parse::{self, Parsed};
use crate::store::Blobs;

const BATCH: u32 = 200;
const IDLE_CHECK: Duration = Duration::from_secs(15 * 60);

pub async fn run(
    db: Db,
    blobs: Arc<Blobs>,
    hub: Arc<Hub>,
    own_domains: HashSet<String>,
    wake: Arc<Notify>,
) {
    let own_domains = Arc::new(own_domains);
    loop {
        match drain(&db, &blobs, &hub, &own_domains).await {
            Ok(_) => {
                let _ = tokio::time::timeout(IDLE_CHECK, wake.notified()).await;
            }
            Err(e) => {
                tracing::warn!("indexing failed: {e:#}");
                tokio::time::sleep(Duration::from_secs(60)).await;
            }
        }
    }
}

async fn drain(
    db: &Db,
    blobs: &Arc<Blobs>,
    hub: &Hub,
    own_domains: &Arc<HashSet<String>>,
) -> Result<usize> {
    let started = Instant::now();
    let mut total = 0;
    loop {
        let pending = db.unparsed(parse::VERSION, BATCH).await?;
        if pending.is_empty() {
            break;
        }
        let blobs = blobs.clone();
        let own_domains = own_domains.clone();
        let parsed = tokio::task::spawn_blocking(move || -> Result<Vec<(String, Parsed)>> {
            pending
                .into_iter()
                .map(|blob| {
                    let raw = blobs.get(&blob)?;
                    let p = parse::parse(&raw, &blob, &own_domains);
                    Ok((blob, p))
                })
                .collect()
        })
        .await??;
        total += parsed.len();
        db.store_parsed(parse::VERSION, &parsed).await?;
        let stored: Vec<String> = parsed.into_iter().map(|(blob, _)| blob).collect();
        hub.changed(db.folders_of_blobs(&stored).await?);
    }
    if total > 0 {
        tracing::info!(
            indexed = total,
            secs = started.elapsed().as_secs_f32(),
            "indexed messages"
        );
    }
    Ok(total)
}
