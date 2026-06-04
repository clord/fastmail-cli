use crate::commands::SearchFilter;
use crate::jmap::authenticated_client;
use crate::models::{Output, emails_to_values, parse_fields, print_emails_jsonl};

pub async fn list_mailboxes() -> anyhow::Result<()> {
    let mut client = authenticated_client().await?;

    let mailboxes = client.list_mailboxes().await?;
    Output::success(mailboxes).print();

    Ok(())
}

pub async fn list_emails(
    mailbox: &str,
    limit: u32,
    offset: u32,
    fields: Option<String>,
    jsonl: bool,
) -> anyhow::Result<()> {
    let mut client = authenticated_client().await?;

    let mailbox = client.find_mailbox(mailbox).await?;
    // Use the filtered search path so we get paging + state + total metadata.
    let filter = SearchFilter::default();
    let result = client
        .search_emails_filtered(&filter, Some(&mailbox.id), limit, offset)
        .await?;
    let fields = parse_fields(fields.as_deref());
    let checkpoint = client.current_email_state().await?;

    if jsonl {
        print_emails_jsonl(&result.emails, fields.as_deref(), Some(&checkpoint));
    } else {
        let values = emails_to_values(&result.emails, fields.as_deref());
        Output::success(serde_json::json!({
            "mailbox": mailbox,
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

pub async fn list_identities() -> anyhow::Result<()> {
    let client = authenticated_client().await?;
    let identities = client.list_identities().await?;
    Output::success(identities).print();
    Ok(())
}
