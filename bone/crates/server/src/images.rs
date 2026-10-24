use std::io::Write;
use std::net::{IpAddr, SocketAddr};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use hmac::{Hmac, KeyInit, Mac};
use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use sha2::{Digest, Sha256};
use sqlx::Row;

use crate::db::Db;
use crate::notify::now;

const MAX_BYTES: usize = 10 * 1024 * 1024;
const RETRY_FAILED: i64 = 24 * 60 * 60;
const USER_AGENT: &str = "Mozilla/5.0";

pub struct Images {
    db: Db,
    dir: PathBuf,
    key: Vec<u8>,
    http: reqwest::Client,
    allow_private: bool,
}

pub struct Image {
    pub content_type: String,
    pub bytes: Vec<u8>,
}

impl Images {
    pub fn open(data_dir: &Path, db: Db) -> Result<Self> {
        Self::build(data_dir, db, false)
    }

    fn build(data_dir: &Path, db: Db, allow_private: bool) -> Result<Self> {
        let dir = data_dir.join("images");
        std::fs::create_dir_all(&dir)?;
        let key_path = data_dir.join("image.key");
        let key = if key_path.exists() {
            std::fs::read(&key_path)?
        } else {
            let (pair, _) = ece::generate_keypair_and_auth_secret()?;
            let key = Sha256::digest(pair.raw_components()?.private_key()).to_vec();
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&key_path)
                .with_context(|| format!("creating {}", key_path.display()))?
                .write_all(&key)?;
            key
        };
        let mut http = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .timeout(Duration::from_secs(15))
            .redirect(reqwest::redirect::Policy::custom(move |attempt| {
                if attempt.previous().len() >= 5 {
                    attempt.error("too many redirects")
                } else if !allow_private && !allowed_url(attempt.url()) {
                    attempt.error("redirect to a private address")
                } else {
                    attempt.follow()
                }
            }));
        if !allow_private {
            http = http.dns_resolver(Arc::new(PublicOnly));
        }
        Ok(Self {
            db,
            dir,
            key,
            http: http.build()?,
            allow_private,
        })
    }

    pub fn id(url: &str) -> String {
        hex::encode(Sha256::digest(url.as_bytes()))
    }

    fn mac(&self, id: &str) -> Hmac<Sha256> {
        let mut mac =
            <Hmac<Sha256> as KeyInit>::new_from_slice(&self.key).expect("hmac takes any key size");
        mac.update(id.as_bytes());
        mac
    }

    pub fn path_for(&self, id: &str) -> String {
        let sig = hex::encode(self.mac(id).finalize().into_bytes());
        format!("/api/img/{id}?sig={sig}")
    }

    pub fn verify(&self, id: &str, sig: &str) -> bool {
        hex::decode(sig).is_ok_and(|s| self.mac(id).verify_slice(&s).is_ok())
    }

    pub async fn register(&self, urls: &[String]) -> Result<Vec<String>> {
        let mut ids = Vec::with_capacity(urls.len());
        for url in urls {
            let id = Self::id(url);
            sqlx::query("INSERT OR IGNORE INTO image (id, url, used_at) VALUES (?, ?, ?)")
                .bind(&id)
                .bind(url)
                .bind(now())
                .execute(&self.db.pool)
                .await?;
            ids.push(id);
        }
        Ok(ids)
    }

    pub async fn get(&self, id: &str) -> Result<Option<Image>> {
        let Some(row) = sqlx::query("SELECT url, content_type, failed_at FROM image WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.db.pool)
            .await?
        else {
            return Ok(None);
        };
        sqlx::query("UPDATE image SET used_at = ? WHERE id = ?")
            .bind(now())
            .bind(id)
            .execute(&self.db.pool)
            .await?;
        if let Some(content_type) = row.get::<Option<String>, _>("content_type")
            && let Ok(bytes) = std::fs::read(self.file(id))
        {
            return Ok(Some(Image {
                content_type,
                bytes,
            }));
        }
        if row
            .get::<Option<i64>, _>("failed_at")
            .is_some_and(|t| now() - t < RETRY_FAILED)
        {
            return Ok(None);
        }
        let url: String = row.get("url");
        match self.fetch(&url).await {
            Ok(image) => {
                self.store(id, &image).await?;
                Ok(Some(image))
            }
            Err(e) => {
                tracing::debug!("image fetch failed: {e:#}");
                sqlx::query("UPDATE image SET failed_at = ? WHERE id = ?")
                    .bind(now())
                    .bind(id)
                    .execute(&self.db.pool)
                    .await?;
                Ok(None)
            }
        }
    }

    pub async fn prefetch(&self, urls: &[String]) -> Result<()> {
        for id in self.register(urls).await? {
            self.get(&id).await?;
        }
        Ok(())
    }

    pub async fn sweep(&self, unused_for: i64) -> Result<usize> {
        let ids: Vec<String> = sqlx::query_scalar("SELECT id FROM image WHERE used_at < ?")
            .bind(now() - unused_for)
            .fetch_all(&self.db.pool)
            .await?;
        for id in &ids {
            let _ = std::fs::remove_file(self.file(id));
            sqlx::query("DELETE FROM image WHERE id = ?")
                .bind(id)
                .execute(&self.db.pool)
                .await?;
        }
        Ok(ids.len())
    }

    async fn fetch(&self, url: &str) -> Result<Image> {
        let parsed = reqwest::Url::parse(url)?;
        if !self.allow_private && !allowed_url(&parsed) {
            bail!("not a public http(s) address");
        }
        let mut res = self.http.get(parsed).send().await?.error_for_status()?;
        let content_type = res
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(|v| {
                v.split(';')
                    .next()
                    .unwrap_or("")
                    .trim()
                    .to_ascii_lowercase()
            })
            .unwrap_or_default();
        if !content_type.starts_with("image/") {
            bail!("not an image: {content_type:?}");
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = res.chunk().await? {
            bytes.extend_from_slice(&chunk);
            if bytes.len() > MAX_BYTES {
                bail!("image is larger than {MAX_BYTES} bytes");
            }
        }
        Ok(Image {
            content_type,
            bytes,
        })
    }

    async fn store(&self, id: &str, image: &Image) -> Result<()> {
        let path = self.file(id);
        std::fs::create_dir_all(path.parent().expect("image paths have a parent"))?;
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, &image.bytes)?;
        std::fs::rename(&tmp, &path)?;
        sqlx::query("UPDATE image SET content_type = ?, size = ?, failed_at = NULL WHERE id = ?")
            .bind(&image.content_type)
            .bind(image.bytes.len() as i64)
            .bind(id)
            .execute(&self.db.pool)
            .await?;
        Ok(())
    }

    fn file(&self, id: &str) -> PathBuf {
        self.dir.join(&id[..2]).join(id)
    }
}

fn allowed_url(url: &reqwest::Url) -> bool {
    if !matches!(url.scheme(), "http" | "https") {
        return false;
    }
    match url.host() {
        Some(url::Host::Ipv4(ip)) => is_public(IpAddr::V4(ip)),
        Some(url::Host::Ipv6(ip)) => is_public(IpAddr::V6(ip)),
        Some(url::Host::Domain(d)) => {
            !d.eq_ignore_ascii_case("localhost") && !d.ends_with(".localhost")
        }
        None => false,
    }
}

pub fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let [a, b, ..] = v4.octets();
            !(v4.is_private()
                || v4.is_loopback()
                || v4.is_link_local()
                || v4.is_broadcast()
                || v4.is_documentation()
                || v4.is_unspecified()
                || v4.is_multicast()
                || a == 0
                || a >= 240
                || (a == 100 && (64..128).contains(&b))
                || (a == 198 && (b == 18 || b == 19))
                || (a == 192 && b == 0 && v4.octets()[2] == 0))
        }
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_public(IpAddr::V4(v4));
            }
            let first = v6.segments()[0];
            !(v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || (first & 0xfe00) == 0xfc00
                || (first & 0xffc0) == 0xfe80
                || first == 0x2001 && v6.segments()[1] == 0x0db8
                || first == 0x0064 && v6.segments()[1] == 0xff9b)
        }
    }
}

struct PublicOnly;

impl Resolve for PublicOnly {
    fn resolve(&self, name: Name) -> Resolving {
        Box::pin(async move {
            let addrs: Vec<SocketAddr> = tokio::net::lookup_host((name.as_str(), 0))
                .await?
                .filter(|a| is_public(a.ip()))
                .collect();
            if addrs.is_empty() {
                return Err(format!("{} has no public address", name.as_str()).into());
            }
            Ok(Box::new(addrs.into_iter()) as Addrs)
        })
    }
}

#[cfg(test)]
mod tests {
    use std::io::Read;

    use super::*;

    #[test]
    fn only_public_addresses_pass() {
        for ip in [
            "127.0.0.1",
            "10.1.2.3",
            "172.16.0.1",
            "192.168.1.1",
            "169.254.169.254",
            "100.64.0.1",
            "100.127.255.255",
            "0.0.0.0",
            "255.255.255.255",
            "198.18.0.1",
            "::1",
            "fd7a:115c:a1e0::1",
            "fe80::1",
            "::ffff:127.0.0.1",
            "64:ff9b::7f00:1",
        ] {
            assert!(!is_public(ip.parse().unwrap()), "{ip}");
        }
        for ip in ["1.1.1.1", "51.210.3.23", "100.128.0.1", "2606:4700::1111"] {
            assert!(is_public(ip.parse().unwrap()), "{ip}");
        }
    }

    #[test]
    fn urls_need_http_and_a_public_host() {
        let ok = |u: &str| allowed_url(&reqwest::Url::parse(u).unwrap());
        assert!(ok("https://img.example.com/a.png"));
        assert!(!ok("http://127.0.0.1:8222/admin"));
        assert!(!ok("http://[::1]/x"));
        assert!(!ok("http://localhost/x"));
        assert!(!ok("file:///etc/passwd"));
    }

    fn temp() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "bone-images-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn serve(responses: Vec<&'static [u8]>) -> (u16, std::thread::JoinHandle<usize>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = std::thread::spawn(move || {
            let mut served = 0;
            for response in responses {
                let (mut stream, _) = listener.accept().unwrap();
                let mut buf = [0u8; 4096];
                let _ = stream.read(&mut buf).unwrap();
                stream.write_all(response).unwrap();
                served += 1;
            }
            served
        });
        (port, handle)
    }

    #[tokio::test]
    async fn signed_paths_verify_and_reject_tampering() {
        let dir = temp();
        let images = Images::open(&dir, Db::memory().await.unwrap()).unwrap();
        let id = Images::id("https://img.example.com/a.png");
        let path = images.path_for(&id);
        let sig = path.split("sig=").nth(1).unwrap();
        assert!(images.verify(&id, sig));
        assert!(!images.verify(&Images::id("https://other.example.com"), sig));
        assert!(!images.verify(&id, "00"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn the_real_client_refuses_local_addresses() {
        let dir = temp();
        let images = Images::open(&dir, Db::memory().await.unwrap()).unwrap();
        let (port, server) = serve(vec![]);
        let ids = images
            .register(&[format!("http://127.0.0.1:{port}/pixel.gif")])
            .await
            .unwrap();
        assert!(images.get(&ids[0]).await.unwrap().is_none());
        drop(server);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn fetches_once_and_serves_from_cache() {
        let dir = temp();
        let images = Images::build(&dir, Db::memory().await.unwrap(), true).unwrap();
        let (port, server) = serve(vec![
            b"HTTP/1.1 200 OK\r\ncontent-type: image/gif\r\ncontent-length: 3\r\nconnection: close\r\n\r\nGIF",
            b"HTTP/1.1 200 OK\r\ncontent-type: text/html\r\ncontent-length: 2\r\nconnection: close\r\n\r\nhi",
        ]);
        let ids = images
            .register(&[
                format!("http://127.0.0.1:{port}/a.gif"),
                format!("http://127.0.0.1:{port}/page"),
            ])
            .await
            .unwrap();
        let first = images.get(&ids[0]).await.unwrap().unwrap();
        assert_eq!(
            (first.content_type.as_str(), first.bytes.as_slice()),
            ("image/gif", &b"GIF"[..])
        );
        assert!(images.get(&ids[1]).await.unwrap().is_none());
        assert_eq!(server.join().unwrap(), 2);
        let again = images.get(&ids[0]).await.unwrap().unwrap();
        assert_eq!(again.bytes, b"GIF");
        assert!(images.get(&ids[1]).await.unwrap().is_none());
        assert!(images.get("unknown").await.unwrap().is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
