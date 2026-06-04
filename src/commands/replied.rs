use crate::commands::thread::latest_message;
use crate::jmap::authenticated_client;
use crate::models::{Email, EmailAddress, Output};

/// Returns true iff the latest message's From email matches (case-insensitively)
/// one of my identity email addresses.
pub(crate) fn is_from_me(latest: Option<&Email>, my_emails: &[String]) -> bool {
    let from_email = latest
        .and_then(|e| e.from.as_ref())
        .and_then(|f| f.first())
        .map(|a| a.email.trim().to_lowercase());

    let Some(from_email) = from_email else {
        return false;
    };

    my_emails
        .iter()
        .any(|m| m.trim().to_lowercase() == from_email)
}

/// Determine whether the latest message in a thread was sent by one of my
/// identities (i.e. I/we already replied).
pub async fn replied(email_id: &str) -> anyhow::Result<()> {
    let client = authenticated_client().await?;

    let emails = client.get_thread(email_id).await?;
    let identities = client.list_identities().await?;
    let my_emails: Vec<String> = identities.into_iter().map(|i| i.email).collect();

    let latest = latest_message(&emails);
    let last_sender: Option<EmailAddress> = latest
        .and_then(|e| e.from.as_ref())
        .and_then(|f| f.first())
        .cloned();
    let replied = is_from_me(latest, &my_emails);

    #[derive(serde::Serialize)]
    struct RepliedResponse {
        replied: bool,
        last_sender: Option<EmailAddress>,
        my_identities: Vec<String>,
    }

    Output::success(RepliedResponse {
        replied,
        last_sender,
        my_identities: my_emails,
    })
    .print();

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn email_from(from: Option<&str>) -> Email {
        let mut obj = serde_json::json!({ "id": "e1" });
        if let Some(f) = from {
            obj["from"] = serde_json::json!([{ "email": f }]);
        }
        serde_json::from_value(obj).unwrap()
    }

    #[test]
    fn test_is_from_me_match() {
        let e = email_from(Some("me@example.com"));
        let mine = vec!["me@example.com".to_string()];
        assert!(is_from_me(Some(&e), &mine));
    }

    #[test]
    fn test_is_from_me_case_insensitive() {
        let e = email_from(Some("Me@Example.COM"));
        let mine = vec!["me@example.com".to_string()];
        assert!(is_from_me(Some(&e), &mine));
    }

    #[test]
    fn test_is_from_me_no_from() {
        let e = email_from(None);
        let mine = vec!["me@example.com".to_string()];
        assert!(!is_from_me(Some(&e), &mine));
    }

    #[test]
    fn test_is_from_me_not_me() {
        let e = email_from(Some("other@example.com"));
        let mine = vec!["me@example.com".to_string()];
        assert!(!is_from_me(Some(&e), &mine));
    }

    #[test]
    fn test_is_from_me_empty_identities() {
        let e = email_from(Some("me@example.com"));
        let mine: Vec<String> = vec![];
        assert!(!is_from_me(Some(&e), &mine));
    }

    #[test]
    fn test_is_from_me_none_latest() {
        let mine = vec!["me@example.com".to_string()];
        assert!(!is_from_me(None, &mine));
    }
}
