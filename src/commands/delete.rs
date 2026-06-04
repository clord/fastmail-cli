use crate::commands::SearchFilter;
use crate::commands::bulk::{print_dry_run, resolve_targets};
use crate::jmap::authenticated_client;
use crate::models::Output;

/// Bulk delete: move emails to Trash (soft) or permanently destroy (`--hard`).
/// Acts on explicit ids OR a filter that resolves the targets.
pub async fn delete_bulk(
    ids: Vec<String>,
    filter: SearchFilter,
    limit: u32,
    hard: bool,
    dry_run: bool,
) -> anyhow::Result<()> {
    let mut client = authenticated_client().await?;

    let targets = resolve_targets(&mut client, ids, &filter, limit).await?;
    let target_ids: Vec<String> = targets.iter().map(|e| e.id.clone()).collect();

    if dry_run {
        let action = if hard {
            "permanently destroy"
        } else {
            "move to Trash"
        };
        print_dry_run(&targets, action);
        return Ok(());
    }

    if hard {
        client.destroy_emails(&target_ids).await?;
        Output::<()>::success_msg(format!(
            "Permanently destroyed {} email(s)",
            target_ids.len()
        ))
        .print();
    } else {
        client.delete_emails(&target_ids).await?;
        Output::<()>::success_msg(format!("Moved {} email(s) to Trash", target_ids.len())).print();
    }

    Ok(())
}
