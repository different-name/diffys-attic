use std::collections::BTreeSet;

use serde::Serialize;
use tokio::sync::broadcast;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Change {
    pub folders: BTreeSet<String>,
}

pub struct Hub {
    tx: broadcast::Sender<Change>,
}

impl Hub {
    pub fn new() -> Self {
        Self {
            tx: broadcast::channel(256).0,
        }
    }

    pub fn changed(&self, folders: impl IntoIterator<Item = String>) {
        let folders: BTreeSet<String> = folders.into_iter().collect();
        if !folders.is_empty() {
            let _ = self.tx.send(Change { folders });
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Change> {
        self.tx.subscribe()
    }
}

pub fn folder_key(role: Option<&str>, path: &str) -> String {
    role.unwrap_or(path).to_string()
}
