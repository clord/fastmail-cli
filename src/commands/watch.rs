use crate::jmap::authenticated_client;
use std::cell::Cell;
use std::time::Duration;

/// Watch the account via JMAP push (EventSource). Prints one StateChange JSON
/// object per line to stdout. Reconnects with exponential backoff until
/// interrupted; with `once`, exits after the first StateChange.
pub async fn watch(types: &str, ping: u32, once: bool) -> anyhow::Result<()> {
    // The server pings every `ping` seconds, so twice that plus slack with no
    // bytes at all means the connection is dead.
    let idle = Duration::from_secs(u64::from(ping) * 2 + 30);
    let mut backoff = 1u64;
    loop {
        let got_event = Cell::new(false);
        let result = async {
            // Re-fetch the session on every (re)connect — cheap, and it keeps
            // a long-lived watcher working across session URL changes.
            let client = authenticated_client().await?;
            let url = client.push_url(types, ping)?;
            client
                .stream_events(&url, idle, |event, data| {
                    if event == "state" {
                        println!("{data}");
                        got_event.set(true);
                        return once;
                    }
                    false
                })
                .await?;
            Ok::<(), anyhow::Error>(())
        }
        .await;

        match result {
            // stream_events only returns Ok when the handler stopped it (--once).
            Ok(()) => return Ok(()),
            Err(e) => {
                if got_event.take() {
                    backoff = 1; // the connection was healthy before it dropped
                }
                eprintln!("watch: {e}; reconnecting in {backoff}s");
                tokio::time::sleep(Duration::from_secs(backoff)).await;
                backoff = (backoff * 2).min(60);
            }
        }
    }
}
