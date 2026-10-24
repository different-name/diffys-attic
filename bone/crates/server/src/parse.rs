use std::collections::HashSet;

use mail_parser::{Address, HeaderValue, Message, MessageParser, MimeHeaders};
use serde::{Deserialize, Serialize};

pub const VERSION: i64 = 1;

const BODY_LIMIT: usize = 64 * 1024;
const SNIPPET_LIMIT: usize = 200;
const ENVELOPE_HEADERS: &[&str] = &["X-Envelope-To", "X-Original-To", "Delivered-To"];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Mailbox {
    pub name: Option<String>,
    pub addr: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Attachment {
    pub name: String,
    pub mime: String,
    pub size: usize,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Parsed {
    pub message_id: String,
    pub parents: Vec<String>,
    pub subject: String,
    pub from: Option<Mailbox>,
    pub to: Vec<Mailbox>,
    pub cc: Vec<Mailbox>,
    pub date: Option<i64>,
    pub recipient: Option<String>,
    pub attachments: Vec<Attachment>,
    pub snippet: String,
    pub body: String,
}

impl Parsed {
    pub fn thread_ids(&self) -> impl Iterator<Item = &str> {
        std::iter::once(self.message_id.as_str()).chain(self.parents.iter().map(String::as_str))
    }
}

pub fn parse(raw: &[u8], blob: &str, own_domains: &HashSet<String>) -> Parsed {
    let Some(msg) = MessageParser::default().parse(raw) else {
        return Parsed {
            message_id: format!("blob:{blob}"),
            ..Default::default()
        };
    };
    let to = mailboxes(msg.to());
    let cc = mailboxes(msg.cc());
    let body = msg
        .body_text(0)
        .map(|t| truncate(&t, BODY_LIMIT).to_string())
        .unwrap_or_default();
    let mut parents = ids(msg.references());
    for id in ids(msg.in_reply_to()) {
        if !parents.contains(&id) {
            parents.push(id);
        }
    }
    Parsed {
        message_id: msg
            .message_id()
            .map(str::to_string)
            .unwrap_or_else(|| format!("blob:{blob}")),
        parents,
        subject: msg.subject().unwrap_or_default().trim().to_string(),
        from: mailboxes(msg.from()).into_iter().next(),
        recipient: recipient(&msg, &to, &cc, own_domains),
        to,
        cc,
        date: msg.date().map(|d| d.to_timestamp()),
        attachments: msg
            .attachments()
            .filter_map(|p| {
                Some(Attachment {
                    name: p.attachment_name()?.to_string(),
                    mime: p
                        .content_type()
                        .map(|c| match c.subtype() {
                            Some(sub) => format!("{}/{}", c.ctype(), sub),
                            None => c.ctype().to_string(),
                        })
                        .unwrap_or_else(|| "application/octet-stream".into()),
                    size: p.contents().len(),
                })
            })
            .collect(),
        snippet: snippet(&body),
        body,
    }
}

fn recipient(
    msg: &Message<'_>,
    to: &[Mailbox],
    cc: &[Mailbox],
    own_domains: &HashSet<String>,
) -> Option<String> {
    for header in ENVELOPE_HEADERS {
        if let Some(addr) = msg
            .header_values(*header)
            .filter_map(HeaderValue::as_text)
            .find_map(first_address)
        {
            return Some(addr);
        }
    }
    to.iter()
        .chain(cc)
        .map(|m| &m.addr)
        .find(|a| {
            a.rsplit_once('@')
                .is_some_and(|(_, d)| own_domains.contains(d))
        })
        .cloned()
}

fn first_address(value: &str) -> Option<String> {
    value
        .split(',')
        .map(|v| {
            v.trim()
                .trim_start_matches('<')
                .trim_end_matches('>')
                .trim()
        })
        .find(|v| v.contains('@') && !v.contains(char::is_whitespace))
        .map(str::to_ascii_lowercase)
}

fn mailboxes(addr: Option<&Address<'_>>) -> Vec<Mailbox> {
    let Some(addr) = addr else {
        return Vec::new();
    };
    addr.iter()
        .filter_map(|a| {
            Some(Mailbox {
                name: a
                    .name()
                    .map(str::trim)
                    .filter(|n| !n.is_empty())
                    .map(str::to_string),
                addr: a.address()?.trim().to_ascii_lowercase(),
            })
        })
        .collect()
}

fn ids(value: &HeaderValue<'_>) -> Vec<String> {
    match value {
        HeaderValue::Text(t) => vec![t.to_string()],
        HeaderValue::TextList(list) => list.iter().map(|t| t.to_string()).collect(),
        _ => Vec::new(),
    }
}

fn snippet(body: &str) -> String {
    let mut out = String::new();
    for word in body
        .lines()
        .filter(|l| !l.trim_start().starts_with('>'))
        .flat_map(str::split_whitespace)
    {
        if out.len() + word.len() + 1 > SNIPPET_LIMIT {
            break;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(word);
    }
    out
}

fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn own() -> HashSet<String> {
        HashSet::from(["example.com".to_string()])
    }

    fn parse_str(raw: &str) -> Parsed {
        parse(raw.replace('\n', "\r\n").as_bytes(), "abc", &own())
    }

    #[test]
    fn catch_all_uses_the_envelope_recipient() {
        let p = parse_str(
            "Delivered-To: catch@example.com\nX-Envelope-To: shop.fox@example.com\nFrom: Shop <news@shop.example.net>\nTo: shop.fox@example.com\nSubject: Sale\nMessage-ID: <1@shop>\n\nhello",
        );
        assert_eq!(p.recipient.as_deref(), Some("shop.fox@example.com"));
        assert_eq!(
            p.from,
            Some(Mailbox {
                name: Some("Shop".into()),
                addr: "news@shop.example.net".into()
            })
        );
        assert_eq!(p.message_id, "1@shop");
    }

    #[test]
    fn forwarded_mail_uses_x_original_to() {
        let p = parse_str(
            "Delivered-To: old@mail.example.org\nX-Original-To: old@mail.example.org\nFrom: a@example.net\nTo: old@mail.example.org\nSubject: x\n\nbody",
        );
        assert_eq!(p.recipient.as_deref(), Some("old@mail.example.org"));
    }

    #[test]
    fn falls_back_to_an_own_address_in_to_or_cc() {
        let p = parse_str(
            "From: me@example.com\nTo: friend@example.net\nCc: Me <Alias@Example.com>\nSubject: x\n\nbody",
        );
        assert_eq!(p.recipient.as_deref(), Some("alias@example.com"));
        let p = parse_str("From: a@example.net\nTo: b@example.net\nSubject: x\n\nbody");
        assert_eq!(p.recipient, None);
    }

    #[test]
    fn collects_thread_parents() {
        let p = parse_str(
            "From: a@example.net\nMessage-ID: <3@x>\nIn-Reply-To: <2@x>\nReferences: <1@x> <2@x>\nSubject: Re: x\n\nbody",
        );
        assert_eq!(p.parents, vec!["1@x", "2@x"]);
        assert_eq!(
            p.thread_ids().collect::<Vec<_>>(),
            vec!["3@x", "1@x", "2@x"]
        );
    }

    #[test]
    fn missing_message_id_falls_back_to_the_blob() {
        let p = parse_str("From: a@example.net\nSubject: x\n\nbody");
        assert_eq!(p.message_id, "blob:abc");
        assert_eq!(parse(b"", "abc", &own()).message_id, "blob:abc");
    }

    #[test]
    fn snippet_skips_quoted_lines() {
        let p = parse_str(
            "From: a@example.net\nSubject: x\n\nSounds good,\n  see you then.\n\n> On Monday you wrote:\n> old text\n",
        );
        assert_eq!(p.snippet, "Sounds good, see you then.");
    }

    #[test]
    fn html_only_mail_gets_text_and_attachments_are_listed() {
        let p = parse_str(concat!(
            "From: a@example.net\nSubject: x\nMIME-Version: 1.0\n",
            "Content-Type: multipart/mixed; boundary=b\n\n",
            "--b\nContent-Type: text/html\n\n<p>Hello <b>there</b></p>\n",
            "--b\nContent-Type: application/pdf\nContent-Disposition: attachment; filename=\"invoice.pdf\"\nContent-Transfer-Encoding: base64\n\naGVsbG8=\n",
            "--b--\n"
        ));
        assert!(p.body.contains("Hello there"), "{:?}", p.body);
        assert_eq!(
            p.attachments,
            vec![Attachment {
                name: "invoice.pdf".into(),
                mime: "application/pdf".into(),
                size: 5
            }]
        );
    }
}
