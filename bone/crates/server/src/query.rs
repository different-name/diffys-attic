#[derive(Debug, Default, PartialEq)]
pub struct Query {
    pub terms: Vec<(Option<&'static str>, String)>,
    pub folder: Option<String>,
    pub unread: Option<bool>,
    pub flagged: bool,
    pub attachment: bool,
    pub before: Option<i64>,
    pub after: Option<i64>,
}

impl Query {
    pub fn parse(input: &str) -> Self {
        let mut q = Query::default();
        for token in input.split_whitespace() {
            let Some((key, value)) = token.split_once(':').filter(|(_, v)| !v.is_empty()) else {
                q.terms.push((None, token.to_string()));
                continue;
            };
            match (
                key.to_ascii_lowercase().as_str(),
                value.to_ascii_lowercase().as_str(),
            ) {
                ("from", _) => q.terms.push((Some("sender"), value.to_string())),
                ("to", _) => q.terms.push((Some("recipients"), value.to_string())),
                ("subject", _) => q.terms.push((Some("subject"), value.to_string())),
                ("in", _) => q.folder = Some(value.to_string()),
                ("is", "unread") => q.unread = Some(true),
                ("is", "read") => q.unread = Some(false),
                ("is", "flagged" | "starred") => q.flagged = true,
                ("has", "attachment" | "attachments") => q.attachment = true,
                ("before", v) if date(v).is_some() => q.before = date(v),
                ("after", v) if date(v).is_some() => q.after = date(v),
                _ => q.terms.push((None, token.to_string())),
            }
        }
        q
    }

    pub fn fts(&self) -> Option<String> {
        if self.terms.is_empty() {
            return None;
        }
        let parts: Vec<String> = self
            .terms
            .iter()
            .map(|(column, term)| {
                let quoted = format!("\"{}\"*", term.replace('"', "\"\""));
                match column {
                    Some(c) => format!("{c} : {quoted}"),
                    None => quoted,
                }
            })
            .collect();
        Some(parts.join(" "))
    }
}

// YYYY-MM-DD as unix seconds at UTC midnight
fn date(s: &str) -> Option<i64> {
    let mut parts = s.splitn(3, '-');
    let y: i64 = parts.next()?.parse().ok()?;
    let m: i64 = parts.next()?.parse().ok()?;
    let d: i64 = parts.next()?.parse().ok()?;
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some((era * 146097 + doe - 719468) * 86400)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_words_become_prefix_terms() {
        let q = Query::parse("invoice  march");
        assert_eq!(q.fts().as_deref(), Some(r#""invoice"* "march"*"#));
        assert_eq!(Query::parse("  ").fts(), None);
    }

    #[test]
    fn operators_become_filters_or_columns() {
        let q = Query::parse(
            "from:github in:Receipts is:unread has:attachment subject:\"deploy\" after:2026-10-01 before:2026-10-24",
        );
        assert_eq!(
            q.fts().as_deref(),
            Some(r#"sender : "github"* subject : """deploy"""*"#)
        );
        assert_eq!(q.folder.as_deref(), Some("Receipts"));
        assert_eq!(q.unread, Some(true));
        assert!(q.attachment);
        assert_eq!(q.after, Some(1_790_812_800));
        assert_eq!(q.before, Some(1_792_800_000));
    }

    #[test]
    fn unknown_operators_and_bad_dates_are_searched_as_text() {
        let q = Query::parse("foo:bar before:yesterday http://x");
        assert_eq!(q.before, None);
        assert_eq!(q.terms.len(), 3);
    }

    #[test]
    fn dates_match_the_unix_epoch() {
        assert_eq!(date("1970-01-01"), Some(0));
        assert_eq!(date("2000-03-01"), Some(951_868_800));
        assert_eq!(date("2026-13-01"), None);
    }
}
