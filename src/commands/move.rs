use crate::jmap::authenticated_client;
use crate::models::Output;

/// Move one or more emails to a mailbox, addressed either by name/path (`--to`)
/// or by mailbox id (`--to-id`). Id is unambiguous when sub-mailboxes share a
/// leaf name across parents.
pub async fn move_email(
    email_ids: &[String],
    to_name: Option<&str>,
    to_id: Option<&str>,
) -> anyhow::Result<()> {
    let mut client = authenticated_client().await?;

    let (mailbox_id, label) = match (to_id, to_name) {
        (Some(id), _) => (id.to_string(), id.to_string()),
        (None, Some(name)) => {
            let mailbox = client.find_mailbox(name).await?;
            (mailbox.id, mailbox.name)
        }
        (None, None) => anyhow::bail!("provide a destination with --to <name> or --to-id <id>"),
    };

    client.move_emails(email_ids, &mailbox_id).await?;

    let msg = if email_ids.len() == 1 {
        format!("Moved email to {}", label)
    } else {
        format!("Moved {} emails to {}", email_ids.len(), label)
    };
    Output::<()>::success_msg(msg).print();

    Ok(())
}
