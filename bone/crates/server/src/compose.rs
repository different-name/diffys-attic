use std::collections::HashSet;

use anyhow::{Context, Result, bail};
use lettre::address::Envelope;
use lettre::message::header::{ContentType, Header, HeaderName, HeaderValue};
use lettre::message::{Mailbox as LettreMailbox, Message};
use lettre::transport::smtp::authentication::Credentials;
use lettre::transport::smtp::extension::ClientId;
use lettre::{AsyncSmtpTransport, AsyncTransport, Tokio1Executor};
use serde::{Deserialize, Serialize};

use crate::config::{AccountConfig, read_secret};
use crate::parse::Mailbox;
use crate::views::MessageMeta;

#[derive(Debug, Default)]
pub struct Own {
    pub addresses: HashSet<String>,
    pub domains: HashSet<String>,
}

impl Own {
    pub fn contains(&self, addr: &str) -> bool {
        let addr = addr.to_ascii_lowercase();
        self.addresses.contains(&addr) || domain(&addr).is_some_and(|d| self.domains.contains(d))
    }
}

pub fn domain(addr: &str) -> Option<&str> {
    addr.rsplit_once('@').map(|(_, d)| d)
}

#[derive(Debug, Serialize, PartialEq)]
pub struct Draft {
    pub from: String,
    pub to: Vec<Mailbox>,
    pub cc: Vec<Mailbox>,
    pub subject: String,
    pub reply_to: i64,
}

pub fn reply(original: &MessageMeta, own: &Own, fallback_from: &str, all: bool) -> Draft {
    let from = match (&original.row.recipient, original.recipient_hosted) {
        (Some(r), true) => r.clone(),
        _ => fallback_from.to_string(),
    };
    let sent_by_me = original
        .row
        .from
        .as_ref()
        .is_some_and(|f| own.contains(&f.addr));
    let (to, mut cc) = if sent_by_me {
        (original.to.clone(), original.cc.clone())
    } else if !original.reply_to.is_empty() {
        (original.reply_to.clone(), Vec::new())
    } else {
        (original.row.from.iter().cloned().collect(), Vec::new())
    };
    if all && !sent_by_me {
        cc.extend(original.to.iter().chain(&original.cc).cloned());
    }
    if !all {
        cc.clear();
    }
    let mut seen: HashSet<String> = to.iter().map(|m| m.addr.clone()).collect();
    cc.retain(|m| !own.contains(&m.addr) && seen.insert(m.addr.clone()));
    Draft {
        from,
        to,
        cc,
        subject: reply_subject(&original.row.subject),
        reply_to: original.row.id,
    }
}

pub fn reply_subject(subject: &str) -> String {
    let trimmed = subject.trim();
    if trimmed
        .get(..3)
        .is_some_and(|p| p.eq_ignore_ascii_case("re:"))
    {
        trimmed.to_string()
    } else {
        format!("Re: {trimmed}")
    }
}

#[derive(Debug, Deserialize)]
pub struct Outgoing {
    pub from: String,
    #[serde(default)]
    pub to: Vec<Mailbox>,
    #[serde(default)]
    pub cc: Vec<Mailbox>,
    #[serde(default)]
    pub bcc: Vec<Mailbox>,
    #[serde(default)]
    pub subject: String,
    #[serde(default)]
    pub text: String,
    pub reply_to: Option<i64>,
    #[serde(default)]
    pub draft: Option<String>,
}

pub struct Threading {
    pub message_id: String,
    pub parents: Vec<String>,
}

fn mailbox(m: &Mailbox) -> Result<LettreMailbox> {
    Ok(LettreMailbox::new(
        m.name.clone(),
        m.addr
            .parse()
            .with_context(|| format!("{} is not a valid address", m.addr))?,
    ))
}

pub fn build(
    out: &Outgoing,
    from: &Mailbox,
    message_id: &str,
    threading: Option<&Threading>,
    keep_bcc: bool,
) -> Result<Message> {
    if out.to.is_empty() && out.cc.is_empty() && out.bcc.is_empty() {
        bail!("add at least one recipient");
    }
    let mut b = Message::builder()
        .from(mailbox(from)?)
        .subject(out.subject.trim())
        .message_id(Some(format!("<{message_id}>")))
        .date_now();
    for m in &out.to {
        b = b.to(mailbox(m)?);
    }
    for m in &out.cc {
        b = b.cc(mailbox(m)?);
    }
    for m in &out.bcc {
        b = b.bcc(mailbox(m)?);
    }
    if let Some(t) = threading {
        b = b.in_reply_to(format!("<{}>", t.message_id));
        let refs: Vec<String> = t
            .parents
            .iter()
            .chain(std::iter::once(&t.message_id))
            .map(|id| format!("<{id}>"))
            .collect();
        b = b.references(refs.join(" "));
    }
    if keep_bcc {
        b = b.keep_bcc();
    }
    Ok(b.header(ContentType::TEXT_PLAIN).body(out.text.clone())?)
}

pub fn draft_id_ok(id: &str) -> bool {
    id.len() <= 200
        && id.contains('@')
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "._-@+=".contains(c))
}

const UNSET_FROM: &str = "X-Bone-Unset-From";

#[derive(Clone)]
struct UnsetFrom;

impl Header for UnsetFrom {
    fn name() -> HeaderName {
        HeaderName::new_from_ascii_str(UNSET_FROM)
    }

    fn parse(_: &str) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        Ok(Self)
    }

    fn display(&self) -> HeaderValue {
        HeaderValue::new(Self::name(), "1".into())
    }
}

pub fn build_draft(
    out: &Outgoing,
    from: Option<&Mailbox>,
    mailbox_login: &Mailbox,
    message_id: &str,
    threading: Option<&Threading>,
) -> Result<Message> {
    let sender = mailbox(from.unwrap_or(mailbox_login))?;
    let mut b = Message::builder()
        .from(sender.clone())
        .subject(out.subject.trim())
        .message_id(Some(format!("<{message_id}>")))
        .date_now();
    for m in out.to.iter().filter_map(|m| mailbox(m).ok()) {
        b = b.to(m);
    }
    for m in out.cc.iter().filter_map(|m| mailbox(m).ok()) {
        b = b.cc(m);
    }
    for m in out.bcc.iter().filter_map(|m| mailbox(m).ok()) {
        b = b.bcc(m);
    }
    if let Some(t) = threading {
        b = b.in_reply_to(format!("<{}>", t.message_id));
        let refs: Vec<String> = t
            .parents
            .iter()
            .chain(std::iter::once(&t.message_id))
            .map(|id| format!("<{id}>"))
            .collect();
        b = b.references(refs.join(" "));
    }
    if from.is_none() {
        b = b.header(UnsetFrom);
    }
    let envelope = Envelope::new(None, vec![sender.email.clone()])?;
    Ok(b.keep_bcc()
        .envelope(envelope)
        .header(ContentType::TEXT_PLAIN)
        .body(out.text.clone())?)
}

#[derive(Debug, Serialize, PartialEq)]
pub struct SavedDraft {
    pub draft: String,
    pub from: Option<String>,
    pub to: Vec<Mailbox>,
    pub cc: Vec<Mailbox>,
    pub bcc: Vec<Mailbox>,
    pub subject: String,
    pub text: String,
    #[serde(skip)]
    pub in_reply_to: Option<String>,
}

pub fn read_draft(raw: &[u8]) -> Option<SavedDraft> {
    use crate::parse::mailboxes;
    let m = mail_parser::MessageParser::default().parse(raw)?;
    let in_reply_to = m.in_reply_to();
    Some(SavedDraft {
        draft: m.message_id()?.to_string(),
        from: if m.header_raw(UNSET_FROM).is_some() {
            None
        } else {
            mailboxes(m.from()).into_iter().next().map(|b| b.addr)
        },
        to: mailboxes(m.to()),
        cc: mailboxes(m.cc()),
        bcc: mailboxes(m.bcc()),
        subject: m.subject().unwrap_or_default().to_string(),
        text: m
            .body_text(0)
            .map(|t| t.replace("\r\n", "\n").trim_end().to_string())
            .unwrap_or_default(),
        in_reply_to: in_reply_to
            .as_text()
            .or_else(|| {
                in_reply_to
                    .as_text_list()
                    .and_then(|l| l.first().map(|s| s.as_ref()))
            })
            .map(str::to_string),
    })
}

pub async fn submit(cfg: &AccountConfig, message: &Message) -> Result<()> {
    let login = read_secret(&cfg.login_file)?;
    let password = read_secret(&cfg.password_file)?;
    let (host, port) = cfg
        .smtp
        .rsplit_once(':')
        .and_then(|(h, p)| Some((h, p.parse().ok()?)))
        .with_context(|| format!("smtp must be host:port, got {}", cfg.smtp))?;
    let hello = domain(&login).unwrap_or("localhost").to_string();
    let transport = AsyncSmtpTransport::<Tokio1Executor>::relay(host)?
        .port(port)
        .hello_name(ClientId::Domain(hello))
        .credentials(Credentials::new(login, password))
        .build();
    transport.send(message.clone()).await?;
    Ok(())
}

const WORDS: &[&str] = &[
    "ant", "bat", "bee", "carp", "crab", "crow", "deer", "dove", "duck", "eel", "elk", "finch",
    "fox", "frog", "gnat", "goat", "hare", "hawk", "heron", "ibis", "jay", "koi", "lark", "lynx",
    "mink", "mole", "moth", "newt", "otter", "owl", "panda", "pike", "puffin", "quail", "raven",
    "robin", "seal", "shrew", "snail", "stoat", "swan", "tern", "toad", "trout", "vole", "wasp",
    "wolf", "wren", "yak",
];

pub fn new_address(
    service: &str,
    domain: &str,
    taken: &HashSet<String>,
    seed: u64,
) -> Option<String> {
    let service: String = service
        .trim()
        .to_ascii_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let service = service.trim_matches('-');
    if service.is_empty() {
        return None;
    }
    (0..WORDS.len()).find_map(|i| {
        let word = WORDS[(seed as usize + i) % WORDS.len()];
        let address = format!("{service}.{word}@{domain}");
        (!taken.contains(&address)).then_some(address)
    })
}

pub fn new_message_id(from: &str) -> String {
    use sha2::{Digest, Sha256};
    let seed = format!(
        "{from}{:?}{}",
        std::time::SystemTime::now(),
        std::process::id()
    );
    let hash = hex::encode(&Sha256::digest(seed.as_bytes())[..16]);
    format!("{hash}@{}", domain(from).unwrap_or("localhost"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::views::MessageRow;

    fn mb(addr: &str) -> Mailbox {
        Mailbox {
            name: None,
            addr: addr.into(),
        }
    }

    fn own() -> Own {
        Own {
            addresses: HashSet::from(["old@mail.example.org".to_string()]),
            domains: HashSet::from(["example.com".to_string()]),
        }
    }

    fn original(
        from: &str,
        to: &[&str],
        cc: &[&str],
        recipient: &str,
        hosted: bool,
    ) -> MessageMeta {
        MessageMeta {
            row: MessageRow {
                id: 7,
                thread: 1,
                date: 0,
                folder: "inbox".into(),
                from: Some(mb(from)),
                subject: "Plans".into(),
                snippet: String::new(),
                recipient: Some(recipient.into()),
                unread: false,
                flagged: false,
                answered: false,
                attachments: false,
            },
            message_id: "1@x".into(),
            to: to.iter().map(|a| mb(a)).collect(),
            cc: cc.iter().map(|a| mb(a)).collect(),
            reply_to: Vec::new(),
            attachment_list: Vec::new(),
            blob: String::new(),
            account: "catch".into(),
            parents: Vec::new(),
            recipient_hosted: hosted,
        }
    }

    #[test]
    fn replies_come_from_the_receiving_address() {
        let o = original(
            "a@example.net",
            &["shop.fox@example.com"],
            &[],
            "shop.fox@example.com",
            true,
        );
        let d = reply(&o, &own(), "catch@example.com", false);
        assert_eq!(d.from, "shop.fox@example.com");
        assert_eq!(d.to, vec![mb("a@example.net")]);
        assert_eq!(d.subject, "Re: Plans");
    }

    #[test]
    fn forwarded_mail_is_answered_from_the_mailbox() {
        let o = original(
            "a@example.net",
            &["old@mail.example.org"],
            &[],
            "old@mail.example.org",
            false,
        );
        assert_eq!(
            reply(&o, &own(), "contact@example.com", false).from,
            "contact@example.com"
        );
    }

    #[test]
    fn reply_all_drops_my_addresses_and_duplicates() {
        let o = original(
            "a@example.net",
            &["me@example.com", "b@example.net", "a@example.net"],
            &["old@mail.example.org", "c@example.net"],
            "me@example.com",
            true,
        );
        let d = reply(&o, &own(), "catch@example.com", true);
        assert_eq!(d.to, vec![mb("a@example.net")]);
        assert_eq!(d.cc, vec![mb("b@example.net"), mb("c@example.net")]);
    }

    #[test]
    fn replying_to_my_own_message_goes_to_its_recipients() {
        let o = original(
            "me@example.com",
            &["b@example.net"],
            &["c@example.net"],
            "b@example.net",
            false,
        );
        let d = reply(&o, &own(), "me@example.com", true);
        assert_eq!(d.to, vec![mb("b@example.net")]);
        assert_eq!(d.cc, vec![mb("c@example.net")]);
    }

    #[test]
    fn reply_to_header_wins() {
        let mut o = original(
            "list@example.net",
            &["me@example.com"],
            &[],
            "me@example.com",
            true,
        );
        o.reply_to = vec![mb("thread@example.net")];
        assert_eq!(
            reply(&o, &own(), "x@example.com", false).to,
            vec![mb("thread@example.net")]
        );
    }

    #[test]
    fn new_addresses_are_clean_and_unused() {
        let taken = HashSet::from(["shop.ant@example.com".to_string()]);
        assert_eq!(
            new_address(" Big Shop! ", "example.com", &taken, 0).as_deref(),
            Some("big-shop.ant@example.com")
        );
        assert_eq!(
            new_address("shop", "example.com", &taken, 0).as_deref(),
            Some("shop.bat@example.com")
        );
        assert_eq!(new_address(" !! ", "example.com", &taken, 0), None);
    }

    #[test]
    fn drafts_round_trip() {
        let out = Outgoing {
            from: String::new(),
            to: vec![
                Mailbox {
                    name: Some("Ann".into()),
                    addr: "ann@example.com".into(),
                },
                Mailbox {
                    name: None,
                    addr: "half@".into(),
                },
            ],
            cc: vec![],
            bcc: vec![Mailbox {
                name: None,
                addr: "quiet@example.com".into(),
            }],
            subject: "Plans ü".into(),
            text: "line one\nline two".into(),
            reply_to: None,
            draft: None,
        };
        let login = Mailbox {
            name: None,
            addr: "me@example.com".into(),
        };
        let threading = Threading {
            message_id: "orig@example.com".into(),
            parents: vec![],
        };
        let raw = build_draft(&out, None, &login, "d1@example.com", Some(&threading))
            .unwrap()
            .formatted();
        let d = read_draft(&raw).unwrap();
        assert_eq!(d.draft, "d1@example.com");
        assert_eq!(d.from, None);
        assert_eq!(d.to.len(), 1);
        assert_eq!(d.to[0].addr, "ann@example.com");
        assert_eq!(d.bcc[0].addr, "quiet@example.com");
        assert_eq!(d.subject, "Plans ü");
        assert_eq!(d.text, "line one\nline two");
        assert_eq!(d.in_reply_to.as_deref(), Some("orig@example.com"));

        let raw = build_draft(&out, Some(&login), &login, "d1@example.com", None)
            .unwrap()
            .formatted();
        assert_eq!(
            read_draft(&raw).unwrap().from.as_deref(),
            Some("me@example.com")
        );
        assert!(draft_id_ok("d1@example.com"));
        assert!(!draft_id_ok("x\"y@example.com"));
    }

    #[test]
    fn subjects_get_one_re() {
        assert_eq!(reply_subject("RE: x"), "RE: x");
        assert_eq!(reply_subject("We’re ready"), "Re: We’re ready");
        assert_eq!(reply_subject(" hello "), "Re: hello");
    }

    #[test]
    fn builds_threaded_plain_text() {
        let out = Outgoing {
            from: "me@example.com".into(),
            to: vec![Mailbox {
                name: Some("Bea".into()),
                addr: "b@example.net".into(),
            }],
            cc: vec![],
            bcc: vec![mb("hidden@example.net")],
            subject: "Re: Plans".into(),
            text: "sounds good".into(),
            reply_to: Some(7),
            draft: None,
        };
        let threading = Threading {
            message_id: "2@x".into(),
            parents: vec!["1@x".into()],
        };
        let from = Mailbox {
            name: Some("Me".into()),
            addr: "me@example.com".into(),
        };
        let raw = String::from_utf8(
            build(&out, &from, "abc@example.com", Some(&threading), false)
                .unwrap()
                .formatted(),
        )
        .unwrap();
        assert!(raw.contains("From: Me <me@example.com>\r\n"), "{raw}");
        assert!(raw.contains("To: Bea <b@example.net>\r\n"));
        assert!(raw.contains("Message-ID: <abc@example.com>\r\n"));
        assert!(raw.contains("In-Reply-To: <2@x>\r\n"));
        assert!(raw.contains("References: <1@x> <2@x>\r\n"));
        assert!(!raw.contains("hidden@example.net"));
        assert!(raw.ends_with("sounds good"));

        let kept = String::from_utf8(
            build(&out, &from, "abc@example.com", Some(&threading), true)
                .unwrap()
                .formatted(),
        )
        .unwrap();
        assert!(kept.contains("Bcc: hidden@example.net\r\n"));
    }

    #[test]
    fn needs_a_recipient_and_valid_addresses() {
        let mut out = Outgoing {
            from: "me@example.com".into(),
            to: vec![],
            cc: vec![],
            bcc: vec![],
            subject: String::new(),
            text: String::new(),
            reply_to: None,
            draft: None,
        };
        let from = mb("me@example.com");
        assert!(build(&out, &from, "a@example.com", None, false).is_err());
        out.to = vec![mb("not an address")];
        assert!(build(&out, &from, "a@example.com", None, false).is_err());
    }
}
