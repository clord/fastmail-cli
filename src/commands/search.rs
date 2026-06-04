use crate::jmap::{EmailChanges, authenticated_client};
use crate::models::{Output, parse_fields, print_emails_jsonl, sender_domain_matches};

/// Search filter matching JMAP Email/query FilterCondition.
///
/// This is the canonical filter type passed into jmap. The clap-facing
/// `FilterArgs` (in main.rs) is converted into this via `From`.
#[derive(Debug, Default, Clone)]
pub struct SearchFilter {
    pub text: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub cc: Option<String>,
    pub bcc: Option<String>,
    pub subject: Option<String>,
    pub body: Option<String>,
    pub mailbox: Option<String>,
    pub has_attachment: bool,
    pub min_size: Option<u32>,
    pub max_size: Option<u32>,
    pub before: Option<String>,
    pub after: Option<String>,
    pub unread: bool,
    pub flagged: bool,
    /// Exact sender-domain match (post-filtered; subdomains excluded).
    pub from_domain: Option<String>,
    /// Keywords that must be present (each becomes a `hasKeyword` leaf).
    pub keyword: Vec<String>,
    /// Keywords that must be absent (each becomes a `notKeyword` leaf).
    pub not_keyword: Vec<String>,
}

impl SearchFilter {
    /// True if no filter field is set — used by bulk commands to decide
    /// whether the user supplied a filter (vs. explicit ids).
    pub fn is_empty(&self) -> bool {
        self.text.is_none()
            && self.from.is_none()
            && self.to.is_none()
            && self.cc.is_none()
            && self.bcc.is_none()
            && self.subject.is_none()
            && self.body.is_none()
            && self.mailbox.is_none()
            && !self.has_attachment
            && self.min_size.is_none()
            && self.max_size.is_none()
            && self.before.is_none()
            && self.after.is_none()
            && !self.unread
            && !self.flagged
            && self.from_domain.is_none()
            && self.keyword.is_empty()
            && self.not_keyword.is_empty()
    }
}

/// Resolve the optional mailbox name in `filter` to a mailbox id.
async fn resolve_mailbox_id(
    client: &mut crate::jmap::JmapClient,
    filter: &SearchFilter,
) -> anyhow::Result<Option<String>> {
    if let Some(ref mailbox_name) = filter.mailbox {
        Ok(Some(client.find_mailbox(mailbox_name).await?.id))
    } else {
        Ok(None)
    }
}

#[allow(clippy::too_many_arguments)]
pub async fn search(
    filter: SearchFilter,
    limit: u32,
    offset: u32,
    fields: Option<String>,
    jsonl: bool,
    since: Option<String>,
) -> anyhow::Result<()> {
    let mut client = authenticated_client().await?;
    let mailbox_id = resolve_mailbox_id(&mut client, &filter).await?;
    let fields = parse_fields(fields.as_deref());

    // Delta / incremental sync mode.
    if let Some(since_state) = since {
        let EmailChanges {
            new_state,
            created,
            updated,
            destroyed,
            ..
        } = client.drain_email_changes(&since_state).await?;

        if jsonl {
            // Metadata line first (state + destroyed), then created/updated emails.
            eprintln!(
                "{}",
                serde_json::json!({"state": new_state, "destroyed": destroyed})
            );
            print_emails_jsonl(&created, fields.as_deref(), None);
            print_emails_jsonl(&updated, fields.as_deref(), None);
        } else {
            let created_v = crate::models::emails_to_values(&created, fields.as_deref());
            let updated_v = crate::models::emails_to_values(&updated, fields.as_deref());
            Output::success(serde_json::json!({
                "state": new_state,
                "created": created_v,
                "updated": updated_v,
                "destroyed": destroyed,
            }))
            .print();
        }
        return Ok(());
    }

    let result = client
        .search_emails_filtered(&filter, mailbox_id.as_deref(), limit, offset)
        .await?;

    let mut emails = result.emails;
    // Exact from-domain post-filter (JMAP `from` is substring-only).
    if let Some(ref domain) = filter.from_domain {
        emails.retain(|e| sender_domain_matches(e, domain));
    }

    // The checkpoint token for `--since` is the Email *collection* state (used
    // by Email/changes), distinct from the query's `queryState`.
    let checkpoint = client.current_email_state().await?;

    if jsonl {
        print_emails_jsonl(&emails, fields.as_deref(), Some(&checkpoint));
    } else {
        let values = crate::models::emails_to_values(&emails, fields.as_deref());
        Output::success(serde_json::json!({
            "state": checkpoint,
            "queryState": result.state,
            "total": result.total,
            "position": result.position,
            "emails": values,
        }))
        .print();
    }

    Ok(())
}

/// Resolve the matching email ids for a filter (used by bulk commands).
pub async fn resolve_filter_ids(
    client: &mut crate::jmap::JmapClient,
    filter: &SearchFilter,
    limit: u32,
) -> anyhow::Result<Vec<crate::models::Email>> {
    let mailbox_id = resolve_mailbox_id(client, filter).await?;
    let result = client
        .search_emails_filtered(filter, mailbox_id.as_deref(), limit, 0)
        .await?;
    let mut emails = result.emails;
    if let Some(ref domain) = filter.from_domain {
        emails.retain(|e| sender_domain_matches(e, domain));
    }
    Ok(emails)
}
