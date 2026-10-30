use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default = "default_bind")]
    pub bind: String,
    #[serde(default = "default_data_dir")]
    pub data_dir: PathBuf,
    #[serde(default)]
    pub cache_dir: Option<PathBuf>,
    #[serde(default)]
    pub accounts: Vec<AccountConfig>,
    #[serde(default)]
    pub devices: Vec<DeviceConfig>,
    #[serde(default)]
    pub web_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceConfig {
    pub name: String,
    pub token_file: PathBuf,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AccountConfig {
    pub name: String,
    #[serde(default = "default_imap")]
    pub imap: String,
    #[serde(default = "default_smtp")]
    pub smtp: String,
    #[serde(default)]
    pub catch_all: bool,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub domains: Vec<String>,
    pub login_file: PathBuf,
    pub password_file: PathBuf,
}

impl Config {
    pub fn load() -> Result<Self> {
        let path = match std::env::var_os("BONE_SERVER_CONFIG") {
            Some(p) => PathBuf::from(p),
            None => xdg("XDG_CONFIG_HOME", ".config").join("bone/server.json"),
        };
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        Self::parse(&text).with_context(|| format!("parsing {}", path.display()))
    }

    pub fn cache_dir(&self) -> &Path {
        self.cache_dir.as_deref().unwrap_or(&self.data_dir)
    }

    fn parse(text: &str) -> Result<Self> {
        Ok(serde_json::from_str(text)?)
    }
}

pub fn read_secret(path: &Path) -> Result<String> {
    let s = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    Ok(s.trim().to_string())
}

fn default_bind() -> String {
    "127.0.0.1:8790".into()
}

fn default_smtp() -> String {
    "smtp.migadu.com:465".into()
}

fn default_imap() -> String {
    "imap.migadu.com:993".into()
}

fn default_data_dir() -> PathBuf {
    xdg("XDG_STATE_HOME", ".local/state").join("bone")
}

fn xdg(var: &str, fallback: &str) -> PathBuf {
    match std::env::var_os(var) {
        Some(v) if !v.is_empty() => PathBuf::from(v),
        _ => PathBuf::from(std::env::var_os("HOME").unwrap_or_else(|| ".".into())).join(fallback),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_full_example() {
        let cfg = Config::parse(
            r#"{
                "bind": "127.0.0.1:9000",
                "data_dir": "/var/lib/bone",
                "cache_dir": "/var/cache/bone",
                "devices": [{ "name": "phone", "token_file": "/run/phone" }],
                "accounts": [
                    { "name": "main", "login_file": "/run/login", "password_file": "/run/pass" },
                    { "name": "catch", "imap": "imap.example.com:993", "login_file": "/run/l2", "password_file": "/run/p2", "catch_all": true }
                ]
            }"#,
        )
        .unwrap();
        assert_eq!(cfg.bind, "127.0.0.1:9000");
        assert_eq!(cfg.data_dir, PathBuf::from("/var/lib/bone"));
        assert_eq!(cfg.cache_dir(), Path::new("/var/cache/bone"));
        assert_eq!(cfg.devices[0].name, "phone");
        assert_eq!(cfg.devices[0].token_file, PathBuf::from("/run/phone"));
        assert_eq!(cfg.accounts.len(), 2);
        assert_eq!(cfg.accounts[0].name, "main");
        assert_eq!(cfg.accounts[0].imap, "imap.migadu.com:993");
        assert_eq!(cfg.accounts[0].login_file, PathBuf::from("/run/login"));
        assert_eq!(cfg.accounts[0].password_file, PathBuf::from("/run/pass"));
        assert_eq!(cfg.accounts[1].imap, "imap.example.com:993");
        assert!(!cfg.accounts[0].catch_all);
        assert!(cfg.accounts[1].catch_all);
    }

    #[test]
    fn rejects_unknown_key() {
        assert!(Config::parse(r#"{ "port": 1 }"#).is_err());
        assert!(
            Config::parse(
                r#"{ "accounts": [{ "name": "a", "login_file": "/l", "password_file": "/p", "password": "x" }] }"#
            )
            .is_err()
        );
    }
}
