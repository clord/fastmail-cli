use crate::commands::SearchFilter;
use crate::commands::bulk::{print_dry_run, resolve_targets};
use crate::jmap::{authenticated_client, compute_keyword_update};
use crate::models::Output;

/// Bulk flag: add (or `--remove`) keywords on emails selected by explicit ids
/// OR a filter. Idempotent: starts from each email's current keyword map so
/// unrelated keywords are never clobbered.
pub async fn flag_bulk(
    ids: Vec<String>,
    filter: SearchFilter,
    keywords: Vec<String>,
    remove: bool,
    limit: u32,
    dry_run: bool,
) -> anyhow::Result<()> {
    if keywords.is_empty() {
        anyhow::bail!("Specify at least one --keyword to add or remove");
    }

    let mut client = authenticated_client().await?;

    let targets = resolve_targets(&mut client, ids, &filter, limit).await?;

    if dry_run {
        let verb = if remove { "remove" } else { "add" };
        print_dry_run(
            &targets,
            &format!("{verb} keyword(s): {}", keywords.join(", ")),
        );
        return Ok(());
    }

    let mut count = 0usize;
    for email in &targets {
        let new_keywords = compute_keyword_update(&email.keywords, &keywords, remove);
        // Skip the network call when nothing would change (idempotency).
        if new_keywords == email.keywords {
            continue;
        }
        client.set_keywords(&email.id, new_keywords).await?;
        count += 1;
    }

    let verb = if remove { "Removed" } else { "Added" };
    Output::<()>::success_msg(format!(
        "{} keyword(s) on {} email(s) ({} unchanged)",
        verb,
        count,
        targets.len() - count
    ))
    .print();
    Ok(())
}
