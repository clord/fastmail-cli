use crate::jmap::authenticated_client;
use crate::models::Output;

pub async fn create_mailbox(name: &str, parent: Option<&str>) -> anyhow::Result<()> {
    let client = authenticated_client().await?;

    let mailbox = client.create_mailbox(name, parent).await?;

    Output::success(mailbox).print();
    Ok(())
}

pub async fn delete_mailbox(id: &str, remove_emails: bool) -> anyhow::Result<()> {
    let client = authenticated_client().await?;

    client.delete_mailbox(id, remove_emails).await?;

    Output::<()>::success_msg(format!("Deleted mailbox {}", id)).print();
    Ok(())
}
