mod config;
mod db;
mod imap;
mod store;
mod sync;
mod watch;

use std::io::IsTerminal;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Result, bail};
use tokio::signal::unix::{SignalKind, signal};
use tracing_subscriber::EnvFilter;

use crate::config::Config;
use crate::db::Db;
use crate::store::Blobs;

const SWEEP_EVERY: Duration = Duration::from_secs(60 * 60);

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_ansi(std::io::stdout().is_terminal())
        .init();

    let config = Config::load()?;
    if config.accounts.is_empty() {
        bail!("no accounts configured");
    }
    std::fs::create_dir_all(&config.data_dir)?;
    let db = Db::open(&config.data_dir.join("bone.db")).await?;
    let blobs = Arc::new(Blobs::open(&config.data_dir)?);

    for account in &config.accounts {
        tokio::spawn(sync::run(account.clone(), db.clone(), blobs.clone()));
    }
    tokio::spawn(sweep(db, blobs));

    let mut term = signal(SignalKind::terminate())?;
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = term.recv() => {}
    }
    Ok(())
}

async fn sweep(db: Db, blobs: Arc<Blobs>) {
    let mut every = tokio::time::interval(SWEEP_EVERY);
    loop {
        every.tick().await;
        let result = async {
            let keep = db.referenced_blobs().await?;
            blobs.sweep(&keep, SWEEP_EVERY)
        }
        .await;
        match result {
            Ok(0) => {}
            Ok(n) => tracing::info!(removed = n, "swept unreferenced messages"),
            Err(e) => tracing::warn!("blob sweep failed: {e:#}"),
        }
    }
}
