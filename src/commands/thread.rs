use std::fmt::Write as _;

use crate::jmap::authenticated_client;
use crate::models::{Email, EmailAddress, Output};
use crate::util::html_to_text;

/// Output format for the `thread` command.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, clap::ValueEnum)]
pub enum ThreadFormat {
    /// Full thread as JSON wrapped in the Output envelope (default).
    #[default]
    Json,
    /// Compact plain-text summary block, printed raw to stdout.
    Digest,
}

/// Max characters kept per message excerpt in a digest.
const SNIPPET_LEN: usize = 280;

/// Pick the most recent message in a thread by `received_at`.
/// ISO-8601 strings sort lexicographically; a `None` received_at is treated
/// as the oldest. On ties the first such message encountered wins.
pub(crate) fn latest_message(emails: &[Email]) -> Option<&Email> {
    emails
        .iter()
        .max_by(|a, b| match (&a.received_at, &b.received_at) {
            (Some(x), Some(y)) => x.cmp(y),
            (Some(_), None) => std::cmp::Ordering::Greater,
            (None, Some(_)) => std::cmp::Ordering::Less,
            (None, None) => std::cmp::Ordering::Equal,
        })
}

/// One-line body excerpt: prefers the plain-text body, falls back to
/// HTML→text, then the server preview. Quoted reply lines (`> …`) are
/// dropped, whitespace collapsed, and the result truncated to SNIPPET_LEN
/// characters.
pub(crate) fn snippet(email: &Email) -> String {
    let raw = email
        .text_content()
        .map(str::to_string)
        .or_else(|| email.html_content().map(html_to_text))
        .or_else(|| email.preview.clone())
        .unwrap_or_default();

    let unquoted = raw
        .lines()
        .filter(|l| !l.trim_start().starts_with('>'))
        .collect::<Vec<_>>()
        .join(" ");
    let collapsed = unquoted.split_whitespace().collect::<Vec<_>>().join(" ");

    let mut out: String = collapsed.chars().take(SNIPPET_LEN).collect();
    if collapsed.chars().count() > SNIPPET_LEN {
        out.push('…');
    }
    out
}

/// Render a whole thread as one compact plain-text summary block: a header
/// (subject, message count, date range, participants) followed by one short
/// entry per message in chronological order.
pub(crate) fn render_digest(emails: &[Email]) -> String {
    // Oldest → newest; `None` received_at sorts first. Stable sort keeps the
    // server's thread order for ties.
    let mut sorted: Vec<&Email> = emails.iter().collect();
    sorted.sort_by(|a, b| a.received_at.cmp(&b.received_at));

    let subject = sorted
        .iter()
        .find_map(|e| e.subject.as_deref())
        .unwrap_or("(no subject)");

    let mut participants: Vec<String> = Vec::new();
    for e in &sorted {
        if let Some(addr) = e.from.as_ref().and_then(|f| f.first()) {
            let s = addr.to_string();
            if !participants.contains(&s) {
                participants.push(s);
            }
        }
    }

    let day = |e: &&Email| -> Option<String> {
        e.received_at
            .as_deref()
            .map(|r| r.chars().take(10).collect())
    };
    let first_day = sorted.first().and_then(day).unwrap_or_else(|| "?".into());
    let last_day = sorted.last().and_then(day).unwrap_or_else(|| "?".into());

    let mut out = String::new();
    let _ = writeln!(out, "Subject: {subject}");
    let _ = writeln!(
        out,
        "Messages: {} | {} → {}",
        sorted.len(),
        first_day,
        last_day
    );
    let _ = writeln!(out, "Participants: {}", participants.join(", "));

    for (i, e) in sorted.iter().enumerate() {
        let from = e
            .from
            .as_ref()
            .and_then(|f| f.first())
            .map(|a| a.to_string())
            .unwrap_or_else(|| "(unknown sender)".into());
        let stamp: String = e
            .received_at
            .as_deref()
            .map(|r| r.chars().take(16).collect::<String>().replace('T', " "))
            .unwrap_or_else(|| "(no date)".into());
        let _ = writeln!(out);
        let _ = writeln!(out, "[{}] {} — {}", i + 1, stamp, from);
        let s = snippet(e);
        if !s.is_empty() {
            let _ = writeln!(out, "    {s}");
        }
    }
    out
}

pub async fn get_thread_fmt(email_id: &str, format: ThreadFormat) -> anyhow::Result<()> {
    let client = authenticated_client().await?;

    let emails = client.get_thread(email_id).await?;
    match format {
        ThreadFormat::Json => Output::success(emails).print(),
        ThreadFormat::Digest => print!("{}", render_digest(&emails)),
    }

    Ok(())
}

/// Output the From of the most recent message in the thread.
pub async fn thread_last_sender(email_id: &str) -> anyhow::Result<()> {
    let client = authenticated_client().await?;
    let emails = client.get_thread(email_id).await?;

    let latest = latest_message(&emails);
    let last_sender: Option<EmailAddress> = latest
        .and_then(|e| e.from.as_ref())
        .and_then(|f| f.first())
        .cloned();
    let received_at: Option<String> = latest.and_then(|e| e.received_at.clone());
    let subject: Option<String> = latest.and_then(|e| e.subject.clone());

    #[derive(serde::Serialize)]
    struct LastSenderResponse {
        last_sender: Option<EmailAddress>,
        received_at: Option<String>,
        subject: Option<String>,
    }

    Output::success(LastSenderResponse {
        last_sender,
        received_at,
        subject,
    })
    .print();

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn email(received_at: Option<&str>) -> Email {
        email_with(received_at, None, None)
    }

    fn email_with(received_at: Option<&str>, from: Option<&str>, subject: Option<&str>) -> Email {
        let mut obj = serde_json::json!({ "id": "e1" });
        if let Some(r) = received_at {
            obj["receivedAt"] = serde_json::json!(r);
        }
        if let Some(f) = from {
            obj["from"] = serde_json::json!([{ "email": f }]);
        }
        if let Some(s) = subject {
            obj["subject"] = serde_json::json!(s);
        }
        serde_json::from_value(obj).unwrap()
    }

    #[test]
    fn test_latest_message_ordering() {
        let emails = vec![
            email(Some("2024-01-01T00:00:00Z")),
            email(Some("2024-03-01T00:00:00Z")),
            email(Some("2024-02-01T00:00:00Z")),
        ];
        let latest = latest_message(&emails).unwrap();
        assert_eq!(latest.received_at.as_deref(), Some("2024-03-01T00:00:00Z"));
    }

    #[test]
    fn test_latest_message_empty() {
        let emails: Vec<Email> = vec![];
        assert!(latest_message(&emails).is_none());
    }

    #[test]
    fn test_latest_message_missing_dates_treated_oldest() {
        let emails = vec![
            email(None),
            email(Some("2020-01-01T00:00:00Z")),
            email(None),
        ];
        let latest = latest_message(&emails).unwrap();
        assert_eq!(latest.received_at.as_deref(), Some("2020-01-01T00:00:00Z"));
    }

    #[test]
    fn test_latest_message_all_missing() {
        let emails = vec![email(None), email(None)];
        // Some message is returned even when all dates are missing.
        assert!(latest_message(&emails).is_some());
    }

    fn email_with_body(
        received_at: &str,
        from: &str,
        subject: Option<&str>,
        text: Option<&str>,
        html: Option<&str>,
    ) -> Email {
        let mut obj = serde_json::json!({
            "id": "e1",
            "receivedAt": received_at,
            "from": [{ "name": null, "email": from }],
        });
        if let Some(s) = subject {
            obj["subject"] = serde_json::json!(s);
        }
        let mut body_values = serde_json::Map::new();
        if let Some(t) = text {
            obj["textBody"] = serde_json::json!([{ "partId": "t1" }]);
            body_values.insert("t1".into(), serde_json::json!({ "value": t }));
        }
        if let Some(h) = html {
            obj["htmlBody"] = serde_json::json!([{ "partId": "h1" }]);
            body_values.insert("h1".into(), serde_json::json!({ "value": h }));
        }
        if !body_values.is_empty() {
            obj["bodyValues"] = serde_json::Value::Object(body_values);
        }
        serde_json::from_value(obj).unwrap()
    }

    #[test]
    fn test_snippet_strips_quoted_lines_and_collapses_whitespace() {
        let e = email_with_body(
            "2026-01-01T00:00:00Z",
            "a@x.com",
            None,
            Some("Sounds good,\n  see you then.\n\n> On Tue, someone wrote:\n> old quoted text"),
            None,
        );
        assert_eq!(snippet(&e), "Sounds good, see you then.");
    }

    #[test]
    fn test_snippet_truncates_on_char_boundary() {
        let long = "é".repeat(SNIPPET_LEN + 10);
        let e = email_with_body("2026-01-01T00:00:00Z", "a@x.com", None, Some(&long), None);
        let s = snippet(&e);
        assert_eq!(s.chars().count(), SNIPPET_LEN + 1); // +1 for the ellipsis
        assert!(s.ends_with('…'));
    }

    #[test]
    fn test_snippet_falls_back_to_html() {
        let e = email_with_body(
            "2026-01-01T00:00:00Z",
            "a@x.com",
            None,
            None,
            Some("<p>Hello <b>world</b></p>"),
        );
        let s = snippet(&e);
        assert!(s.contains("Hello"), "got: {s}");
        assert!(s.contains("world"), "got: {s}");
        assert!(!s.contains('<'), "html tags must be stripped: {s}");
    }

    #[test]
    fn test_render_digest_orders_and_dedupes_participants() {
        let emails = vec![
            email_with_body(
                "2026-02-01T08:00:00Z",
                "bob@y.com",
                Some("Re: Plan"),
                Some("Reply text"),
                None,
            ),
            email_with_body(
                "2026-01-01T09:00:00Z",
                "alice@x.com",
                Some("Plan"),
                Some("Original text"),
                None,
            ),
            email_with_body(
                "2026-03-01T10:00:00Z",
                "alice@x.com",
                Some("Re: Plan"),
                Some("Final text"),
                None,
            ),
        ];
        let d = render_digest(&emails);
        // Header: earliest subject, count, date range, deduped participants.
        assert!(d.starts_with("Subject: Plan\n"), "got: {d}");
        assert!(
            d.contains("Messages: 3 | 2026-01-01 → 2026-03-01"),
            "got: {d}"
        );
        assert!(
            d.contains("Participants: alice@x.com, bob@y.com"),
            "got: {d}"
        );
        // Chronological numbering.
        let p1 = d.find("[1] 2026-01-01").unwrap();
        let p2 = d.find("[2] 2026-02-01").unwrap();
        let p3 = d.find("[3] 2026-03-01").unwrap();
        assert!(p1 < p2 && p2 < p3, "got: {d}");
        assert!(d.contains("Original text"), "got: {d}");
    }

    #[test]
    fn test_render_digest_empty_thread() {
        let d = render_digest(&[]);
        assert!(d.contains("Subject: (no subject)"), "got: {d}");
        assert!(d.contains("Messages: 0"), "got: {d}");
    }

    #[test]
    fn test_latest_message_ties() {
        let a = email_with(Some("2024-05-05T00:00:00Z"), None, Some("first"));
        let b = email_with(Some("2024-05-05T00:00:00Z"), None, Some("second"));
        let emails = vec![a, b];
        let latest = latest_message(&emails).unwrap();
        // A tie still yields one of the tied messages.
        assert_eq!(latest.received_at.as_deref(), Some("2024-05-05T00:00:00Z"));
    }
}
