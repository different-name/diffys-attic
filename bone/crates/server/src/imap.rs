use anyhow::{Context, Result};
use async_imap::types::{Flag, Name, NameAttribute};
use tokio::net::TcpStream;
use tokio_native_tls::{TlsConnector, TlsStream, native_tls};

use crate::config::{AccountConfig, read_secret};

pub type Session = async_imap::Session<TlsStream<TcpStream>>;

pub async fn connect(cfg: &AccountConfig) -> Result<Session> {
    let login = read_secret(&cfg.login_file)?;
    let password = read_secret(&cfg.password_file)?;
    let host = cfg
        .imap
        .rsplit_once(':')
        .map_or(cfg.imap.as_str(), |(h, _)| h);
    let tcp = TcpStream::connect(&cfg.imap)
        .await
        .with_context(|| format!("connecting to {}", cfg.imap))?;
    let tls = TlsConnector::from(native_tls::TlsConnector::new()?)
        .connect(host, tcp)
        .await?;
    let mut client = async_imap::Client::new(tls);
    client
        .read_response()
        .await?
        .context("connection closed before the greeting")?;
    let mut session = client
        .login(&login, &password)
        .await
        .map_err(|(e, _)| e)
        .context("logging in")?;
    session
        .run_command_and_check_ok("ENABLE CONDSTORE UTF8=ACCEPT")
        .await?;
    Ok(session)
}

pub fn role(name: &Name) -> Option<&'static str> {
    if name.name().eq_ignore_ascii_case("INBOX") {
        return Some("inbox");
    }
    name.attributes().iter().find_map(|a| match a {
        NameAttribute::All => Some("all"),
        NameAttribute::Archive => Some("archive"),
        NameAttribute::Drafts => Some("drafts"),
        NameAttribute::Flagged => Some("flagged"),
        NameAttribute::Junk => Some("junk"),
        NameAttribute::Sent => Some("sent"),
        NameAttribute::Trash => Some("trash"),
        _ => None,
    })
}

pub fn selectable(name: &Name) -> bool {
    !name
        .attributes()
        .iter()
        .any(|a| matches!(a, NameAttribute::NoSelect))
}

pub fn flags<'a>(flags: impl Iterator<Item = Flag<'a>>) -> String {
    let mut out: Vec<String> = flags
        .filter_map(|f| match f {
            Flag::Seen => Some("\\Seen".into()),
            Flag::Answered => Some("\\Answered".into()),
            Flag::Flagged => Some("\\Flagged".into()),
            Flag::Deleted => Some("\\Deleted".into()),
            Flag::Draft => Some("\\Draft".into()),
            Flag::Custom(s) => Some(s.into_owned()),
            Flag::Recent | Flag::MayCreate => None,
        })
        .collect();
    out.sort();
    out.join(" ")
}

pub fn uid_set(uids: &[u32]) -> String {
    let mut uids = uids.to_vec();
    uids.sort_unstable();
    uids.dedup();
    let mut parts = Vec::new();
    let mut i = 0;
    while i < uids.len() {
        let start = uids[i];
        let mut end = start;
        while i + 1 < uids.len() && uids[i + 1] == end + 1 {
            i += 1;
            end = uids[i];
        }
        parts.push(if start == end {
            start.to_string()
        } else {
            format!("{start}:{end}")
        });
        i += 1;
    }
    parts.join(",")
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use super::*;

    #[test]
    fn uid_sets_collapse_runs() {
        assert_eq!(uid_set(&[]), "");
        assert_eq!(uid_set(&[7]), "7");
        assert_eq!(uid_set(&[5, 1, 2, 3, 9, 10, 3]), "1:3,5,9:10");
    }

    #[test]
    fn flags_are_sorted_and_skip_session_flags() {
        let f = flags(
            [
                Flag::Seen,
                Flag::Recent,
                Flag::Custom(Cow::Borrowed("$Forwarded")),
                Flag::Flagged,
            ]
            .into_iter(),
        );
        assert_eq!(f, "$Forwarded \\Flagged \\Seen");
    }
}
