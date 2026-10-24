use std::borrow::Cow;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use ammonia::{Builder, UrlRelative};
use url::Url;

pub const IMAGE_SCHEME: &str = "bone-img";

#[derive(Debug, PartialEq)]
pub struct Cleaned {
    pub html: String,
    pub images: Vec<String>,
}

const STYLE_PROPERTIES: &[&str] = &[
    "background-color",
    "border",
    "border-bottom",
    "border-collapse",
    "border-color",
    "border-left",
    "border-radius",
    "border-right",
    "border-spacing",
    "border-style",
    "border-top",
    "border-width",
    "color",
    "direction",
    "display",
    "font-family",
    "font-size",
    "font-style",
    "font-weight",
    "height",
    "letter-spacing",
    "line-height",
    "margin",
    "margin-bottom",
    "margin-left",
    "margin-right",
    "margin-top",
    "max-height",
    "max-width",
    "min-width",
    "padding",
    "padding-bottom",
    "padding-left",
    "padding-right",
    "padding-top",
    "table-layout",
    "text-align",
    "text-decoration",
    "text-transform",
    "vertical-align",
    "white-space",
    "width",
];

const TRACKING_PARAMS: &[&str] = &[
    "_hsenc",
    "_hsmi",
    "dclid",
    "fbclid",
    "gclid",
    "igshid",
    "mc_cid",
    "mc_eid",
    "mkt_tok",
    "msclkid",
    "oly_anon_id",
    "oly_enc_id",
    "vero_conv",
    "vero_id",
    "yclid",
];

pub fn clean_html(html: &str) -> Cleaned {
    let images = Arc::new(Mutex::new(Vec::new()));
    let found = images.clone();
    let html = Builder::default()
        .add_tags(["center", "font"])
        .add_clean_content_tags(["title"])
        .add_generic_attributes(["style", "align", "valign", "width", "height", "bgcolor"])
        .add_tag_attributes("font", ["color", "face", "size"])
        .filter_style_properties(STYLE_PROPERTIES.iter().copied().collect())
        .url_schemes(HashSet::from([
            "http",
            "https",
            "mailto",
            "tel",
            "cid",
            "data",
            IMAGE_SCHEME,
        ]))
        .url_relative(UrlRelative::Deny)
        .link_rel(Some("noopener noreferrer"))
        .set_tag_attribute_value("a", "target", "_blank")
        .attribute_filter(
            move |element, attribute, value| match (element, attribute) {
                ("img", "src") => image_src(value, &found),
                (_, "href") if value.starts_with("http") => Some(clean_link(value).into()),
                (_, "href") if value.starts_with("mailto:") || value.starts_with("tel:") => {
                    Some(value.into())
                }
                (_, "href" | "src") => None,
                _ => Some(value.into()),
            },
        )
        .clean(html)
        .to_string();
    let images = std::mem::take(&mut *images.lock().unwrap());
    Cleaned { html, images }
}

fn image_src<'u>(value: &'u str, found: &Mutex<Vec<String>>) -> Option<Cow<'u, str>> {
    let lower = value.trim_start().to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        let mut found = found.lock().unwrap();
        let index = match found.iter().position(|u| u == value) {
            Some(i) => i,
            None => {
                found.push(value.to_string());
                found.len() - 1
            }
        };
        Some(format!("{IMAGE_SCHEME}:{index}").into())
    } else if lower.starts_with("cid:") || lower.starts_with("data:image/") {
        Some(value.into())
    } else {
        None
    }
}

pub fn clean_link(link: &str) -> String {
    let Ok(mut url) = Url::parse(link) else {
        return link.to_string();
    };
    if let Some(target) = unwrap_redirect(&url) {
        url = target;
    }
    let kept: Vec<(String, String)> = url
        .query_pairs()
        .filter(|(k, _)| !is_tracking(k))
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    if kept.len() != url.query_pairs().count() {
        if kept.is_empty() {
            url.set_query(None);
        } else {
            url.query_pairs_mut().clear().extend_pairs(kept);
        }
    }
    url.to_string()
}

fn is_tracking(key: &str) -> bool {
    key.starts_with("utm_") || TRACKING_PARAMS.contains(&key)
}

fn unwrap_redirect(url: &Url) -> Option<Url> {
    let host = url.host_str()?;
    let path = url.path();
    let param = if host.ends_with(".safelinks.protection.outlook.com") {
        "url"
    } else if (host == "www.google.com" || host == "google.com") && path == "/url" {
        "q"
    } else if (host == "l.facebook.com" || host == "l.instagram.com") && path == "/l.php" {
        "u"
    } else if host == "www.youtube.com" && path == "/redirect" {
        "q"
    } else if host == "steamcommunity.com" && path.starts_with("/linkfilter") {
        "u"
    } else {
        return None;
    };
    let target = url
        .query_pairs()
        .find(|(k, _)| k == param || (param == "u" && k == "url"))?
        .1;
    let target = Url::parse(&target).ok()?;
    matches!(target.scheme(), "http" | "https").then_some(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_scripts_handlers_and_remote_css() {
        let c = clean_html(
            r#"<style>p{color:red}</style><script>alert(1)</script><p onclick="x()" style="color: red; position: fixed; background: url(https://t.example.com/p.gif)">hi</p><link rel="stylesheet" href="https://example.com/a.css"><form><input></form><iframe src="https://example.com"></iframe>"#,
        );
        assert_eq!(c.html, r#"<p style="color:red">hi</p>"#);
        assert!(c.images.is_empty());
    }

    #[test]
    fn drops_the_document_title() {
        let c = clean_html(
            r#"<html><head><title>Your order</title></head><body><p>hi</p></body></html>"#,
        );
        assert_eq!(c.html, "<p>hi</p>");
    }

    #[test]
    fn remote_images_become_indexed_placeholders() {
        let c = clean_html(
            r#"<img src="https://img.example.com/a.png"><img src="cid:logo@x"><img src="https://img.example.com/a.png"><img src="https://t.example.com/open.gif" width="1" height="1"><img src="javascript:alert(1)">"#,
        );
        assert_eq!(
            c.images,
            vec![
                "https://img.example.com/a.png",
                "https://t.example.com/open.gif"
            ]
        );
        assert_eq!(
            c.html,
            r#"<img src="bone-img:0"><img src="cid:logo@x"><img src="bone-img:0"><img src="bone-img:1" width="1" height="1"><img>"#
        );
    }

    #[test]
    fn an_image_placeholder_in_the_source_is_dropped() {
        let c = clean_html(r#"<img src="bone-img:0">"#);
        assert_eq!(c.html, "<img>");
    }

    #[test]
    fn links_open_safely_without_tracking() {
        let c = clean_html(
            r#"<a href="https://shop.example.com/item?id=4&utm_source=news&fbclid=abc">x</a><a href="javascript:alert(1)">y</a><a href="mailto:a@example.com">z</a>"#,
        );
        assert_eq!(
            c.html,
            r#"<a href="https://shop.example.com/item?id=4" target="_blank" rel="noopener noreferrer">x</a><a target="_blank" rel="noopener noreferrer">y</a><a href="mailto:a@example.com" target="_blank" rel="noopener noreferrer">z</a>"#
        );
    }

    #[test]
    fn keeps_newsletter_layout() {
        let c = clean_html(
            r##"<table width="600" align="center" bgcolor="#ffffff"><tr><td valign="top" style="padding: 8px; font-family: Arial"><font color="red">x</font><center>y</center></td></tr></table>"##,
        );
        assert_eq!(
            c.html,
            r##"<table width="600" align="center" bgcolor="#ffffff"><tbody><tr><td valign="top" style="padding:8px;font-family:Arial"><font color="red">x</font><center>y</center></td></tr></tbody></table>"##
        );
    }

    #[test]
    fn unwraps_redirects_that_carry_their_target() {
        assert_eq!(
            clean_link(
                "https://eur01.safelinks.protection.outlook.com/?url=https%3A%2F%2Fexample.com%2Fdoc%3Futm_medium%3Demail%26p%3D2&data=05%7C01&reserved=0"
            ),
            "https://example.com/doc?p=2"
        );
        assert_eq!(
            clean_link("https://www.google.com/url?q=https://example.com/&sa=D"),
            "https://example.com/"
        );
        assert_eq!(
            clean_link("https://steamcommunity.com/linkfilter/?u=https%3A%2F%2Fexample.com%2Fx"),
            "https://example.com/x"
        );
        assert_eq!(
            clean_link("https://www.google.com/url?q=javascript:alert(1)"),
            "https://www.google.com/url?q=javascript:alert(1)"
        );
    }

    #[test]
    fn leaves_clean_links_alone() {
        assert_eq!(
            clean_link("https://example.com/a?b=1&c=2#frag"),
            "https://example.com/a?b=1&c=2#frag"
        );
        assert_eq!(
            clean_link("https://example.com/?utm_source=x"),
            "https://example.com/"
        );
        assert_eq!(clean_link("not a url"), "not a url");
    }

    #[test]
    #[ignore = "needs BONE_CORPUS pointing at a data dir"]
    fn corpus() {
        let dir =
            std::path::PathBuf::from(std::env::var_os("BONE_CORPUS").unwrap()).join("blobs");
        let (mut messages, mut html, mut with_images, mut images) = (0, 0, 0, 0);
        let (mut bytes_in, mut bytes_out) = (0, 0);
        let mut slowest = std::time::Duration::ZERO;
        let started = std::time::Instant::now();
        for shard in std::fs::read_dir(dir).unwrap() {
            for entry in std::fs::read_dir(shard.unwrap().path()).unwrap() {
                let raw = std::fs::read(entry.unwrap().path()).unwrap();
                messages += 1;
                let Some(msg) = mail_parser::MessageParser::default().parse(&raw) else {
                    continue;
                };
                let Some(body) = msg.body_html(0) else {
                    continue;
                };
                html += 1;
                let t = std::time::Instant::now();
                let c = clean_html(&body);
                slowest = slowest.max(t.elapsed());
                bytes_in += body.len();
                bytes_out += c.html.len();
                if !c.images.is_empty() {
                    with_images += 1;
                    images += c.images.len();
                }
            }
        }
        println!(
            "messages {messages}, html {html}, with remote images {with_images} ({images} images), \
             html {bytes_in} -> {bytes_out} bytes, slowest clean {slowest:?}, total {:?}",
            started.elapsed()
        );
    }
}
