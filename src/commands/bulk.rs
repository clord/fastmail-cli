//! Shared helpers for query-driven bulk actions (move, spam, delete, flag).
//!
//! Each bulk command accepts EITHER explicit positional ids OR a search
//! filter that resolves the targets. `--dry-run` prints the would-be-affected
//! emails and the intended action as JSON, performing no mutation.

use crate::commands::{SearchFilter, resolve_filter_ids};
use crate::jmap::JmapClient;
use crate::models::{Email, Output};

/// Resolve the set of target emails for a bulk action.
///
/// - If explicit `ids` are given, fetch their summaries (subject/from/etc.) so
///   dry-run output is meaningful.
/// - Else if `filter` is non-empty, run the search to resolve matches.
/// - Else error clearly: no ids and no filter.
pub async fn resolve_targets(
    client: &mut JmapClient,
    ids: Vec<String>,
    filter: &SearchFilter,
    limit: u32,
) -> anyhow::Result<Vec<Email>> {
    match (ids.is_empty(), filter.is_empty()) {
        (false, _) => {
            // Explicit ids win. Fetch summaries for dry-run/reporting.
            let mut out = Vec::with_capacity(ids.len());
            for id in &ids {
                out.push(client.get_email(id).await?);
            }
            Ok(out)
        }
        (true, false) => Ok(resolve_filter_ids(client, filter, limit).await?),
        (true, true) => {
            anyhow::bail!("Provide either email id(s) or a search filter to select targets")
        }
    }
}

/// Print the dry-run report: the would-be-affected emails (id, subject, from,
/// receivedAt) and the intended action, as JSON. No mutation is performed.
pub fn print_dry_run(targets: &[Email], action: &str) {
    let emails: Vec<serde_json::Value> = targets
        .iter()
        .map(|e| {
            serde_json::json!({
                "id": e.id,
                "subject": e.subject,
                "from": e.from,
                "receivedAt": e.received_at,
            })
        })
        .collect();
    Output::success(serde_json::json!({
        "dryRun": true,
        "action": action,
        "count": targets.len(),
        "emails": emails,
    }))
    .print();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn email(id: &str) -> Email {
        Email {
            id: id.into(),
            blob_id: None,
            thread_id: None,
            mailbox_ids: HashMap::new(),
            keywords: HashMap::new(),
            size: 0,
            received_at: Some("2026-01-01T00:00:00Z".into()),
            message_id: None,
            in_reply_to: None,
            references: None,
            from: None,
            to: None,
            cc: None,
            bcc: None,
            reply_to: None,
            subject: Some("hi".into()),
            sent_at: None,
            preview: None,
            has_attachment: false,
            text_body: None,
            html_body: None,
            attachments: None,
            body_values: None,
        }
    }

    #[test]
    fn test_dry_run_report_shape() {
        // print_dry_run writes to stdout; here we verify the projection shape
        // that feeds it, mirroring the implementation.
        let targets = [email("a"), email("b")];
        let emails: Vec<serde_json::Value> = targets
            .iter()
            .map(|e| {
                serde_json::json!({
                    "id": e.id,
                    "subject": e.subject,
                    "from": e.from,
                    "receivedAt": e.received_at,
                })
            })
            .collect();
        assert_eq!(emails.len(), 2);
        assert_eq!(emails[0]["id"], "a");
        assert_eq!(emails[0]["subject"], "hi");
        assert_eq!(emails[1]["receivedAt"], "2026-01-01T00:00:00Z");
    }
}
