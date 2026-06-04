use crate::jmap::authenticated_client;
use crate::models::Output;
use serde::Serialize;

/// A hyperlink extracted from an email body.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct Link {
    pub text: String,
    pub url: String,
}

/// Query parameter names commonly used by redirect/tracking wrappers to carry
/// the real destination URL.
const REDIRECT_PARAMS: &[&str] = &[
    "url",
    "u",
    "redirect",
    "target",
    "destination",
    "dest",
    "link",
    "out",
    "to",
];

/// Host substrings that strongly suggest a click-tracking / redirect host whose
/// query string wraps the real destination.
const TRACKER_HOST_HINTS: &[&str] = &[
    "click.",
    "email.",
    "links.",
    "link.",
    "list-manage.com",
    "sendgrid.net",
    "mailchimp",
    "customeriomail",
    "sparkpostmail",
    "mandrillapp",
    "mailgun",
    "cmail",
    "hubspotlinks",
    "clicks.",
];

/// Strip a single layer of tracking/redirect wrapping from `url`, returning the
/// real destination when one can be recovered. Otherwise returns the URL
/// unchanged. Applied up to a small fixed depth (never infinitely recursive).
pub fn untrack(url: &str) -> String {
    let mut current = url.to_string();
    // Unwrap at most a few layers to handle nested redirects without looping.
    for _ in 0..3 {
        match unwrap_once(&current) {
            Some(inner) if inner != current => current = inner,
            _ => break,
        }
    }
    current
}

/// Attempt to unwrap a single redirect layer. Returns the inner URL if one is
/// found, else `None`.
fn unwrap_once(url: &str) -> Option<String> {
    let parsed = url::Url::parse(url).ok()?;

    let host = parsed.host_str().unwrap_or("").to_lowercase();
    let is_tracker_host = TRACKER_HOST_HINTS.iter().any(|h| host.contains(h));

    // First, look for an explicit redirect query parameter whose value is an
    // http(s) URL. `Url` already percent-decodes query values for us.
    for (key, value) in parsed.query_pairs() {
        let key_lc = key.to_lowercase();
        if REDIRECT_PARAMS.iter().any(|p| *p == key_lc) && looks_like_http_url(&value) {
            return Some(value.into_owned());
        }
    }

    // For known tracker hosts, also accept *any* query value that is itself an
    // http(s) URL — some wrappers use opaque parameter names.
    if is_tracker_host {
        for (_key, value) in parsed.query_pairs() {
            if looks_like_http_url(&value) {
                return Some(value.into_owned());
            }
        }
    }

    None
}

/// Whether `s` (already percent-decoded) looks like an absolute http(s) URL.
fn looks_like_http_url(s: &str) -> bool {
    let lc = s.trim().to_ascii_lowercase();
    lc.starts_with("http://") || lc.starts_with("https://")
}

/// Parse `html` and extract all `<a href>` hyperlinks as `{ text, url }` pairs,
/// stripping tracking redirects and deduplicating identical pairs. Pure.
pub fn extract_links(html: &str) -> Vec<Link> {
    use scraper::{Html, Selector};

    let doc = Html::parse_document(html);
    let selector = Selector::parse("a[href]").expect("static selector is valid");

    let mut seen = std::collections::HashSet::new();
    let mut links = Vec::new();

    for el in doc.select(&selector) {
        let href = match el.value().attr("href") {
            Some(h) => h.trim(),
            None => continue,
        };
        // Skip empty, anchor, and non-navigational hrefs.
        if href.is_empty()
            || href.starts_with('#')
            || href.starts_with("mailto:")
            || href.starts_with("tel:")
            || href.starts_with("javascript:")
        {
            continue;
        }

        let url = untrack(href);
        let text = normalize_ws(&el.text().collect::<String>());

        let link = Link { text, url };
        if seen.insert(link.clone()) {
            links.push(link);
        }
    }

    links
}

/// Extract bare http(s) URLs from plain text, as a fallback when there is no
/// HTML body. Anchor text is set to the (untracked) URL itself.
pub fn extract_links_from_text(text: &str) -> Vec<Link> {
    let mut seen = std::collections::HashSet::new();
    let mut links = Vec::new();

    for raw in text.split(|c: char| c.is_whitespace() || c == '<' || c == '>' || c == '"') {
        let token = raw.trim_end_matches(['.', ',', ')', ']', '!', ';']);
        if !looks_like_http_url(token) {
            continue;
        }
        let url = untrack(token);
        let link = Link {
            text: url.clone(),
            url,
        };
        if seen.insert(link.clone()) {
            links.push(link);
        }
    }

    links
}

/// Collapse runs of whitespace into single spaces and trim.
fn normalize_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `links <id>` command: fetch the email and emit its hyperlinks.
pub async fn extract_email_links(email_id: &str) -> anyhow::Result<()> {
    let client = authenticated_client().await?;
    let email = client.get_email(email_id).await?;

    let links = if let Some(html) = email.html_content() {
        extract_links(html)
    } else if let Some(text) = email.text_content() {
        extract_links_from_text(text)
    } else {
        Vec::new()
    };

    Output::success(links).print();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_untrack_plain_url_passthrough() {
        let url = "https://example.com/article?id=42&ref=home";
        assert_eq!(untrack(url), url);
    }

    #[test]
    fn test_untrack_generic_url_param_encoded() {
        let wrapped =
            "https://r.example.com/redirect?url=https%3A%2F%2Freal.example.org%2Fpost%2F1";
        assert_eq!(untrack(wrapped), "https://real.example.org/post/1");
    }

    #[test]
    fn test_untrack_generic_url_param_plain() {
        let wrapped = "https://r.example.com/redirect?url=https://real.example.org/x";
        assert_eq!(untrack(wrapped), "https://real.example.org/x");
    }

    #[test]
    fn test_untrack_short_u_param() {
        let wrapped = "https://t.co/go?u=https%3A%2F%2Fdest.example.com%2Fpage";
        assert_eq!(untrack(wrapped), "https://dest.example.com/page");
    }

    #[test]
    fn test_untrack_mailchimp_list_manage() {
        // Mailchimp wraps the destination in a tracking host; opaque-named param.
        let wrapped = "https://example.us1.list-manage.com/track/click?e=abc&id=def&u=https%3A%2F%2Fwww.target.com%2Fblog";
        assert_eq!(untrack(wrapped), "https://www.target.com/blog");
    }

    #[test]
    fn test_untrack_sendgrid_tracker_host_opaque_param() {
        // SendGrid often uses opaque param names but a recognizable host.
        let wrapped = "https://u123.ct.sendgrid.net/wf/click?upn=https%3A%2F%2Fnews.site.com%2Fa";
        assert_eq!(untrack(wrapped), "https://news.site.com/a");
    }

    #[test]
    fn test_untrack_redirect_param_name() {
        let wrapped = "https://click.foo.com/c?redirect=https%3A%2F%2Fbar.com%2Fz";
        assert_eq!(untrack(wrapped), "https://bar.com/z");
    }

    #[test]
    fn test_untrack_target_param_name() {
        let wrapped = "https://links.foo.com/c?target=https%3A%2F%2Fbar.com%2Ft";
        assert_eq!(untrack(wrapped), "https://bar.com/t");
    }

    #[test]
    fn test_untrack_no_inner_url_passthrough() {
        // Tracker host but no embedded URL: leave it alone.
        let url = "https://click.foo.com/c?e=abc&id=123";
        assert_eq!(untrack(url), url);
    }

    #[test]
    fn test_untrack_non_url_input_passthrough() {
        assert_eq!(untrack("not a url"), "not a url");
        assert_eq!(untrack("/relative/path"), "/relative/path");
    }

    #[test]
    fn test_untrack_nested_redirect() {
        // Outer wraps an inner wrapper; unwrap both layers.
        let inner = "https://r.example.com/redirect?url=https%3A%2F%2Ffinal.example.org%2Fp";
        let encoded_inner = "https%3A%2F%2Fr.example.com%2Fredirect%3Furl%3Dhttps%253A%252F%252Ffinal.example.org%252Fp";
        let outer = format!("https://click.foo.com/c?url={encoded_inner}");
        assert_eq!(untrack(&outer), "https://final.example.org/p");
        // sanity: the inner alone unwraps to the final too
        assert_eq!(untrack(inner), "https://final.example.org/p");
    }

    #[test]
    fn test_extract_links_basic() {
        let html = r#"<html><body>
            <a href="https://example.com/a">First</a>
            <a href="https://example.com/b">Second</a>
        </body></html>"#;
        let links = extract_links(html);
        assert_eq!(links.len(), 2);
        assert_eq!(links[0].text, "First");
        assert_eq!(links[0].url, "https://example.com/a");
        assert_eq!(links[1].text, "Second");
    }

    #[test]
    fn test_extract_links_untracks() {
        let html = r#"<a href="https://example.us1.list-manage.com/track/click?u=https%3A%2F%2Fwww.target.com%2Fblog">Read more</a>"#;
        let links = extract_links(html);
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].text, "Read more");
        assert_eq!(links[0].url, "https://www.target.com/blog");
    }

    #[test]
    fn test_extract_links_dedupes() {
        let html = r#"
            <a href="https://example.com/x">Same</a>
            <a href="https://example.com/x">Same</a>
        "#;
        let links = extract_links(html);
        assert_eq!(links.len(), 1);
    }

    #[test]
    fn test_extract_links_skips_anchors_and_mailto() {
        let html = r##"
            <a href="#section">Jump</a>
            <a href="mailto:foo@example.com">Email</a>
            <a href="https://example.com/real">Real</a>
        "##;
        let links = extract_links(html);
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].url, "https://example.com/real");
    }

    #[test]
    fn test_extract_links_normalizes_whitespace() {
        let html = "<a href=\"https://example.com\">  Click\n   here  </a>";
        let links = extract_links(html);
        assert_eq!(links[0].text, "Click here");
    }

    #[test]
    fn test_extract_links_empty_html() {
        assert!(extract_links("<p>no links here</p>").is_empty());
    }

    #[test]
    fn test_extract_links_from_text() {
        let text = "Check https://example.com/a and also https://example.com/b.";
        let links = extract_links_from_text(text);
        assert_eq!(links.len(), 2);
        assert_eq!(links[0].url, "https://example.com/a");
        assert_eq!(links[1].url, "https://example.com/b");
    }

    #[test]
    fn test_extract_links_from_text_untracks() {
        let text = "Visit https://click.foo.com/c?url=https%3A%2F%2Freal.com%2Fx now";
        let links = extract_links_from_text(text);
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].url, "https://real.com/x");
    }

    #[test]
    fn test_extract_links_from_text_dedupes() {
        let text = "https://example.com/x https://example.com/x";
        let links = extract_links_from_text(text);
        assert_eq!(links.len(), 1);
    }
}
