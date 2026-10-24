use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::{Deserialize, Serialize};
use web_push::{
    ContentEncoding, SubscriptionInfo, Urgency, VapidSignatureBuilder, WebPushMessageBuilder,
};

const TTL: u32 = 24 * 60 * 60;
// web push payloads are capped at 4096 bytes after encryption overhead
const MAX_PAYLOAD: usize = 3000;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Subscription {
    pub endpoint: String,
    pub p256dh: String,
    pub auth: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Notice {
    New {
        id: i64,
        from: String,
        subject: String,
        recipient: Option<String>,
    },
}

impl Notice {
    pub fn payload(&self) -> Vec<u8> {
        let mut notice = self.clone();
        loop {
            let bytes = serde_json::to_vec(&notice).expect("notices always serialize");
            let Notice::New { subject, .. } = &mut notice;
            if bytes.len() <= MAX_PAYLOAD || subject.is_empty() {
                return bytes;
            }
            let keep = subject.chars().count() / 2;
            *subject = subject.chars().take(keep).collect();
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum Delivery {
    Sent,
    Gone,
}

pub struct Pusher {
    key: String,
    public: String,
    http: reqwest::Client,
}

impl Pusher {
    pub fn open(data_dir: &Path) -> Result<Self> {
        let path = data_dir.join("vapid.key");
        let key = if path.exists() {
            std::fs::read_to_string(&path)
                .with_context(|| format!("reading {}", path.display()))?
                .trim()
                .to_string()
        } else {
            let (pair, _) = ece::generate_keypair_and_auth_secret()?;
            let key = URL_SAFE_NO_PAD.encode(scalar(pair.raw_components()?.private_key()));
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&path)
                .with_context(|| format!("creating {}", path.display()))?;
            file.write_all(key.as_bytes())?;
            key
        };
        let public = URL_SAFE_NO_PAD.encode(
            VapidSignatureBuilder::from_base64_no_sub(&key)
                .context("vapid.key is not a valid key")?
                .get_public_key(),
        );
        Ok(Self {
            key,
            public,
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(20))
                .build()?,
        })
    }

    pub fn public_key(&self) -> &str {
        &self.public
    }

    pub async fn send(&self, sub: &Subscription, payload: &[u8]) -> Result<Delivery> {
        let info = SubscriptionInfo::new(
            sub.endpoint.as_str(),
            sub.p256dh.as_str(),
            sub.auth.as_str(),
        );
        let signature = VapidSignatureBuilder::from_base64(&self.key, &info)?.build()?;
        let mut builder = WebPushMessageBuilder::new(&info);
        builder.set_payload(ContentEncoding::Aes128Gcm, payload);
        builder.set_vapid_signature(signature);
        builder.set_ttl(TTL);
        builder.set_urgency(Urgency::High);
        let message = builder.build()?;
        let payload = message.payload.context("message has no payload")?;

        let mut req = self
            .http
            .post(&sub.endpoint)
            .header("TTL", TTL.to_string())
            .header("Urgency", "high")
            .header("Content-Encoding", payload.content_encoding.to_str())
            .header("Content-Type", "application/octet-stream");
        for (k, v) in &payload.crypto_headers {
            req = req.header(*k, v);
        }
        let res = req.body(payload.content).send().await?;
        match res.status().as_u16() {
            200..=299 => Ok(Delivery::Sent),
            404 | 410 => Ok(Delivery::Gone),
            code => bail!("push endpoint answered {code}"),
        }
    }
}

// the scalar can come back with its leading zero byte dropped, the vapid signer wants exactly 32
fn scalar(d: &[u8]) -> [u8; 32] {
    let mut out = [0u8; 32];
    out[32 - d.len()..].copy_from_slice(d);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_scalars_are_left_padded() {
        let short = [7u8; 31];
        let padded = scalar(&short);
        assert_eq!(padded[0], 0);
        assert_eq!(&padded[1..], &short[..]);
        assert!(VapidSignatureBuilder::from_base64_no_sub(&URL_SAFE_NO_PAD.encode(padded)).is_ok());
    }

    #[test]
    fn long_subjects_are_shortened_to_fit() {
        let notice = Notice::New {
            id: 1,
            from: "Shop".into(),
            subject: "ü".repeat(5000),
            recipient: Some("a@example.com".into()),
        };
        let payload = notice.payload();
        assert!(payload.len() <= MAX_PAYLOAD);
        let v: serde_json::Value = serde_json::from_slice(&payload).unwrap();
        assert_eq!(v["type"], "new");
        assert_eq!(v["from"], "Shop");
    }

    #[tokio::test]
    async fn encrypts_for_the_subscriber() {
        use std::io::{Read, Write as _};

        let dir = std::env::temp_dir().join(format!("bone-push-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let pusher = Pusher::open(&dir).unwrap();
        assert_eq!(
            Pusher::open(&dir).unwrap().public_key(),
            pusher.public_key()
        );

        let (client, auth) = ece::generate_keypair_and_auth_secret().unwrap();
        let components = client.raw_components().unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = Vec::new();
            let mut chunk = [0u8; 8192];
            loop {
                let n = stream.read(&mut chunk).unwrap();
                buf.extend_from_slice(&chunk[..n]);
                let text = String::from_utf8_lossy(&buf);
                if let Some(end) = text.find("\r\n\r\n") {
                    let len: usize = text[..end]
                        .lines()
                        .find_map(|l| {
                            l.to_ascii_lowercase()
                                .strip_prefix("content-length: ")
                                .map(|v| v.trim().parse().unwrap())
                        })
                        .unwrap();
                    if buf.len() >= end + 4 + len {
                        let head = text[..end].to_string();
                        let body = buf[end + 4..end + 4 + len].to_vec();
                        stream
                            .write_all(b"HTTP/1.1 201 Created\r\ncontent-length: 0\r\n\r\n")
                            .unwrap();
                        return (head, body);
                    }
                }
            }
        });

        let sub = Subscription {
            endpoint: format!("http://127.0.0.1:{port}/up/abc"),
            p256dh: URL_SAFE_NO_PAD.encode(components.public_key()),
            auth: URL_SAFE_NO_PAD.encode(auth),
        };
        let delivery = pusher.send(&sub, b"{\"type\":\"new\"}").await.unwrap();
        assert_eq!(delivery, Delivery::Sent);
        let (head, body) = server.join().unwrap();
        let head = head.to_ascii_lowercase();
        assert!(head.contains("content-encoding: aes128gcm"), "{head}");
        assert!(head.contains(&format!("k={}", pusher.public_key().to_ascii_lowercase())));
        assert_eq!(
            ece::decrypt(&components, &auth, &body).unwrap(),
            b"{\"type\":\"new\"}"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}
