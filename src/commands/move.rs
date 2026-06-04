use crate::commands::SearchFilter;
use crate::commands::bulk::{print_dry_run, resolve_targets};
use crate::jmap::authenticated_client;
use crate::models::Output;

/// Bulk move: act on explicit ids OR a filter that resolves the targets.
/// Destination is resolved by name OR by id (`--to-id`).
pub async fn move_bulk(
    ids: Vec<String>,
    filter: SearchFilter,
    to_name: Option<String>,
    to_id: Option<String>,
    limit: u32,
    dry_run: bool,
) -> anyhow::Result<()> {
    let mut client = authenticated_client().await?;

    let dest_id = match (to_name, to_id) {
        (_, Some(id)) => id,
        (Some(name), None) => client.find_mailbox(&name).await?.id,
        (None, None) => anyhow::bail!("Specify a destination with --to <name> or --to-id <id>"),
    };

    let targets = resolve_targets(&mut client, ids, &filter, limit).await?;

    if dry_run {
        print_dry_run(&targets, &format!("move to mailbox {dest_id}"));
        return Ok(());
    }

    let target_ids: Vec<String> = targets.iter().map(|e| e.id.clone()).collect();
    client.move_emails(&target_ids, &dest_id).await?;

    Output::<()>::success_msg(format!(
        "Moved {} email(s) to {}",
        target_ids.len(),
        dest_id
    ))
    .print();
    Ok(())
}
