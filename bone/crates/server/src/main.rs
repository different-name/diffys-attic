mod actions;
mod api;
mod clean;
mod compose;
mod config;
mod db;
mod hub;
mod images;
mod imap;
mod index;
mod notify;
mod parse;
mod push;
mod query;
mod store;
mod sync;
#[cfg(test)]
mod testutil;
mod views;
mod watch;

use std::collections::{HashMap, HashSet};
use std::io::IsTerminal;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use tokio::signal::unix::{SignalKind, signal};
use tokio::sync::Notify;
use tracing_subscriber::EnvFilter;

use crate::config::{Config, read_secret};
use crate::db::Db;
use crate::hub::Hub;
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
    let blobs = Arc::new(Blobs::open(config.cache_dir())?);

    let mut own_domains = HashSet::new();
    for account in &config.accounts {
        let login = read_secret(&account.login_file)?;
        if let Some((_, domain)) = login.rsplit_once('@') {
            own_domains.insert(domain.to_ascii_lowercase());
        }
    }
    let hub = Arc::new(Hub::new());
    let pusher = Arc::new(push::Pusher::open(&config.data_dir)?);
    let images = Arc::new(images::Images::open(
        &config.data_dir,
        config.cache_dir(),
        db.clone(),
    )?);
    let notifier = Arc::new(notify::Notifier {
        db: db.clone(),
        hub: hub.clone(),
        pusher: pusher.clone(),
        images: images.clone(),
        blobs: blobs.clone(),
    });
    let wake = Arc::new(Notify::new());
    tokio::spawn(index::run(
        db.clone(),
        blobs.clone(),
        hub.clone(),
        notifier,
        own_domains,
        wake.clone(),
    ));

    let mut accounts = HashMap::new();
    let mut outbound = HashMap::new();
    for account in &config.accounts {
        let (tx, rx) = tokio::sync::mpsc::channel(64);
        accounts.insert(account.name.clone(), tx);
        outbound.insert(
            account.name.clone(),
            api::Outbound {
                config: account.clone(),
                login: read_secret(&account.login_file)?,
            },
        );
        tokio::spawn(sync::run(
            account.clone(),
            db.clone(),
            blobs.clone(),
            hub.clone(),
            wake.clone(),
            rx,
        ));
    }
    tokio::spawn(sweep(db.clone(), blobs.clone(), images.clone()));

    if config.devices.is_empty() {
        tracing::warn!("no devices configured, the API will refuse every request");
    } else {
        let names: Vec<&str> = config.devices.iter().map(|d| d.name.as_str()).collect();
        tracing::info!("api devices: {}", names.join(", "));
    }
    let tokens = config
        .devices
        .iter()
        .map(|d| read_secret(&d.token_file).map(|t| api::hash_token(&t)))
        .collect::<Result<Vec<_>>>()?;
    let app = api::App {
        db,
        blobs,
        hub,
        tokens: Arc::new(tokens),
        accounts: Arc::new(accounts),
        outbound: Arc::new(outbound),
        pusher,
        images,
    };
    let listener = tokio::net::TcpListener::bind(&config.bind)
        .await
        .with_context(|| format!("binding {}", config.bind))?;
    tracing::info!("listening on {}", config.bind);
    tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, api::router(app)).await {
            tracing::error!("api server stopped: {e}");
        }
    });

    let mut term = signal(SignalKind::terminate())?;
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = term.recv() => {}
    }
    Ok(())
}

async fn sweep(db: Db, blobs: Arc<Blobs>, images: Arc<images::Images>) {
    let mut every = tokio::time::interval(SWEEP_EVERY);
    loop {
        every.tick().await;
        let result = async {
            let pruned = db.prune_parsed().await?;
            db.prune_notified(notify::now() - 30 * 24 * 60 * 60).await?;
            images.sweep(90 * 24 * 60 * 60).await?;
            let keep = db.referenced_blobs().await?;
            Ok::<_, anyhow::Error>((pruned, blobs.sweep(&keep, SWEEP_EVERY)?))
        }
        .await;
        match result {
            Ok((0, 0)) => {}
            Ok((pruned, removed)) => {
                tracing::info!(pruned, removed, "swept unreferenced messages")
            }
            Err(e) => tracing::warn!("blob sweep failed: {e:#}"),
        }
    }
}
