use crate::commands::SearchFilter;
use crate::commands::bulk::{print_dry_run, resolve_targets};
use crate::jmap::authenticated_client;
use crate::models::Output;

/// Bulk spam: act on explicit ids OR a filter that resolves the targets.
pub async fn spam_bulk(
    ids: Vec<String>,
    filter: SearchFilter,
    limit: u32,
    dry_run: bool,
) -> anyhow::Result<()> {
    let mut client = authenticated_client().await?;

    let targets = resolve_targets(&mut client, ids, &filter, limit).await?;

    if dry_run {
        print_dry_run(&targets, "mark as spam (move to Junk)");
        return Ok(());
    }

    let junk = client.find_mailbox("junk").await?;
    let target_ids: Vec<String> = targets.iter().map(|e| e.id.clone()).collect();
    for id in &target_ids {
        client.move_email(id, &junk.id).await?;
    }

    Output::<()>::success_msg(format!("Marked {} email(s) as spam", target_ids.len())).print();
    Ok(())
}
