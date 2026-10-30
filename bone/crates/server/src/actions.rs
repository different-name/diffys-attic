use std::collections::BTreeSet;

use anyhow::{Result, bail};
use tokio::sync::oneshot;

pub const SEEN: &str = "\\Seen";
pub const FLAGGED: &str = "\\Flagged";
pub const DELETED: &str = "\\Deleted";
pub const ANSWERED: &str = "\\Answered";
pub const DRAFT: &str = "\\Draft";

#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    Flags {
        add: Vec<&'static str>,
        remove: Vec<&'static str>,
    },
    Move(String),
    Delete,
    Append(Vec<u8>),
    SaveDraft {
        raw: Vec<u8>,
        message_id: String,
    },
    DiscardDraft(String),
}

impl Op {
    pub fn parse(name: &str, folder: Option<&str>) -> Result<Self> {
        Ok(match name {
            "read" => Op::Flags {
                add: vec![SEEN],
                remove: vec![],
            },
            "unread" => Op::Flags {
                add: vec![],
                remove: vec![SEEN],
            },
            "flag" => Op::Flags {
                add: vec![FLAGGED],
                remove: vec![],
            },
            "unflag" => Op::Flags {
                add: vec![],
                remove: vec![FLAGGED],
            },
            "archive" | "junk" | "inbox" => Op::Move(name.to_string()),
            "move" => match folder.map(str::trim).filter(|f| !f.is_empty()) {
                Some(f) => Op::Move(f.to_string()),
                None => bail!("move needs a folder"),
            },
            "delete" => Op::Delete,
            _ => bail!("unknown action {name}"),
        })
    }
}

#[derive(Debug)]
pub struct Command {
    pub folder_id: i64,
    pub uids: Vec<u32>,
    pub op: Op,
    pub reply: oneshot::Sender<Result<(), String>>,
}

pub fn edit_flags(flags: &str, add: &[&str], remove: &[&str]) -> String {
    let mut set: BTreeSet<&str> = flags.split(' ').filter(|f| !f.is_empty()).collect();
    set.extend(add.iter().copied());
    for f in remove {
        set.remove(f);
    }
    set.into_iter().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_actions() {
        assert_eq!(
            Op::parse("read", None).unwrap(),
            Op::Flags {
                add: vec![SEEN],
                remove: vec![]
            }
        );
        assert_eq!(
            Op::parse("archive", None).unwrap(),
            Op::Move("archive".into())
        );
        assert_eq!(
            Op::parse("move", Some("Receipts")).unwrap(),
            Op::Move("Receipts".into())
        );
        assert!(Op::parse("move", Some(" ")).is_err());
        assert!(Op::parse("explode", None).is_err());
    }

    #[test]
    fn flag_edits_keep_a_sorted_set() {
        assert_eq!(edit_flags("", &[SEEN], &[]), "\\Seen");
        assert_eq!(
            edit_flags("$Forwarded \\Seen", &[FLAGGED, SEEN], &[]),
            "$Forwarded \\Flagged \\Seen"
        );
        assert_eq!(edit_flags("\\Flagged \\Seen", &[], &[SEEN]), "\\Flagged");
    }
}
