use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub capabilities: HashMap<String, serde_json::Value>,
    pub accounts: HashMap<String, Account>,
    pub primary_accounts: HashMap<String, String>,
    pub username: String,
    pub api_url: String,
    pub download_url: String,
    pub upload_url: String,
    #[serde(default)]
    pub event_source_url: Option<String>,
    #[serde(default)]
    pub state: Option<String>,
}

impl Session {
    pub fn primary_account_id(&self) -> Option<&str> {
        self.primary_accounts
            .get("urn:ietf:params:jmap:mail")
            .map(String::as_str)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub name: String,
    pub is_personal: bool,
    pub is_read_only: bool,
    #[serde(default)]
    pub account_capabilities: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EmailAddress {
    #[serde(default)]
    pub name: Option<String>,
    pub email: String,
}

impl std::fmt::Display for EmailAddress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.name {
            Some(name) if !name.is_empty() => write!(f, "{} <{}>", name, self.email),
            _ => write!(f, "{}", self.email),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Mailbox {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub parent_id: Option<String>,
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub total_emails: u32,
    #[serde(default)]
    pub unread_emails: u32,
    #[serde(default)]
    pub total_threads: u32,
    #[serde(default)]
    pub unread_threads: u32,
    #[serde(default)]
    pub sort_order: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmailBodyPart {
    pub part_id: Option<String>,
    #[serde(default)]
    pub blob_id: Option<String>,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(rename = "type", default)]
    pub content_type: Option<String>,
    #[serde(default)]
    pub charset: Option<String>,
    #[serde(default)]
    pub disposition: Option<String>,
    #[serde(default)]
    pub cid: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmailBodyValue {
    pub value: String,
    #[serde(default)]
    pub is_encoding_problem: bool,
    #[serde(default)]
    pub is_truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Email {
    pub id: String,
    #[serde(default)]
    pub blob_id: Option<String>,
    #[serde(default)]
    pub thread_id: Option<String>,
    #[serde(default)]
    pub mailbox_ids: HashMap<String, bool>,
    #[serde(default)]
    pub keywords: HashMap<String, bool>,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub received_at: Option<String>,
    #[serde(default)]
    pub message_id: Option<Vec<String>>,
    #[serde(default)]
    pub in_reply_to: Option<Vec<String>>,
    #[serde(default)]
    pub references: Option<Vec<String>>,
    #[serde(default)]
    pub from: Option<Vec<EmailAddress>>,
    #[serde(default)]
    pub to: Option<Vec<EmailAddress>>,
    #[serde(default)]
    pub cc: Option<Vec<EmailAddress>>,
    #[serde(default)]
    pub bcc: Option<Vec<EmailAddress>>,
    #[serde(default)]
    pub reply_to: Option<Vec<EmailAddress>>,
    #[serde(default)]
    pub subject: Option<String>,
    #[serde(default)]
    pub sent_at: Option<String>,
    #[serde(default)]
    pub preview: Option<String>,
    #[serde(default)]
    pub has_attachment: bool,
    #[serde(default)]
    pub text_body: Option<Vec<EmailBodyPart>>,
    #[serde(default)]
    pub html_body: Option<Vec<EmailBodyPart>>,
    #[serde(default)]
    pub attachments: Option<Vec<EmailBodyPart>>,
    #[serde(default)]
    pub body_values: Option<HashMap<String, EmailBodyValue>>,
}

impl Email {
    pub fn is_unread(&self) -> bool {
        !self.keywords.contains_key("$seen")
    }

    pub fn is_flagged(&self) -> bool {
        self.keywords.contains_key("$flagged")
    }

    pub fn is_draft(&self) -> bool {
        self.keywords.contains_key("$draft")
    }

    pub fn text_content(&self) -> Option<&str> {
        let body_values = self.body_values.as_ref()?;
        let text_body = self.text_body.as_ref()?;
        let part = text_body.first()?;
        let part_id = part.part_id.as_ref()?;
        body_values.get(part_id).map(|v| v.value.as_str())
    }

    pub fn html_content(&self) -> Option<&str> {
        let body_values = self.body_values.as_ref()?;
        let html_body = self.html_body.as_ref()?;
        let part = html_body.first()?;
        let part_id = part.part_id.as_ref()?;
        body_values.get(part_id).map(|v| v.value.as_str())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Identity {
    pub id: String,
    pub name: String,
    pub email: String,
    #[serde(default)]
    pub reply_to: Option<Vec<EmailAddress>>,
    #[serde(default)]
    pub bcc: Option<Vec<EmailAddress>>,
    #[serde(default)]
    pub text_signature: Option<String>,
    #[serde(default)]
    pub html_signature: Option<String>,
    #[serde(default)]
    pub may_delete: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MaskedEmail {
    pub id: String,
    pub email: String,
    /// One of: pending, enabled, disabled, deleted
    #[serde(default)]
    pub state: Option<String>,
    #[serde(default)]
    pub for_domain: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub last_message_at: Option<String>,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub created_by: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Output<T: Serialize> {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// Extract the domain part (after the last `@`) of an email address.
///
/// Returns `None` if there is no `@` or the domain part is empty. The result
/// is a borrowed slice of the input; case is preserved (callers compare
/// case-insensitively).
pub fn email_domain(addr: &str) -> Option<&str> {
    let at = addr.rfind('@')?;
    let domain = &addr[at + 1..];
    if domain.is_empty() {
        None
    } else {
        Some(domain)
    }
}

/// True if any of the email's `from` addresses has a domain that EXACTLY
/// equals `domain` (case-insensitive). Subdomains do NOT match: a `domain`
/// of `example.com` will not match a sender at `mail.example.com`.
///
/// Emails with no `from` field (or no addresses) never match.
pub fn sender_domain_matches(email: &Email, domain: &str) -> bool {
    let Some(addrs) = email.from.as_ref() else {
        return false;
    };
    addrs
        .iter()
        .any(|a| email_domain(&a.email).is_some_and(|d| d.eq_ignore_ascii_case(domain)))
}

/// Project a JSON object down to only the requested top-level keys.
///
/// Pure helper used by `--fields`. If `value` is an object, returns a new
/// object containing only the keys present in `fields` (preserving the order
/// given in `fields`). Keys absent from the source object are skipped. If the
/// value is not an object it is returned unchanged.
pub fn project_fields(value: &serde_json::Value, fields: &[String]) -> serde_json::Value {
    match value.as_object() {
        Some(map) => {
            let mut out = serde_json::Map::new();
            for f in fields {
                if let Some(v) = map.get(f) {
                    out.insert(f.clone(), v.clone());
                }
            }
            serde_json::Value::Object(out)
        }
        None => value.clone(),
    }
}

/// Serialize each email to a JSON value, optionally projecting down to a
/// subset of top-level fields. Pure helper shared by search / list output.
pub fn emails_to_values(emails: &[Email], fields: Option<&[String]>) -> Vec<serde_json::Value> {
    emails
        .iter()
        .map(|e| {
            let v = serde_json::to_value(e).unwrap_or(serde_json::Value::Null);
            match fields {
                Some(f) => project_fields(&v, f),
                None => v,
            }
        })
        .collect()
}

/// Parse a comma-separated `--fields` argument into a list of field names,
/// trimming whitespace and dropping empty entries. Returns `None` for an
/// absent argument so callers can distinguish "no projection" from "empty".
pub fn parse_fields(arg: Option<&str>) -> Option<Vec<String>> {
    arg.map(|s| {
        s.split(',')
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty())
            .collect()
    })
}

/// Print emails as JSON Lines (one compact object per line). When `fields` is
/// set, each line is projected to the requested keys. The optional `state`
/// token is printed to stderr as a leading metadata line so it does not
/// pollute the emails-as-lines contract on stdout.
pub fn print_emails_jsonl(emails: &[Email], fields: Option<&[String]>, state: Option<&str>) {
    if let Some(state) = state {
        eprintln!("{{\"state\":{}}}", serde_json::Value::String(state.into()));
    }
    for v in emails_to_values(emails, fields) {
        match serde_json::to_string(&v) {
            Ok(line) => println!("{line}"),
            Err(e) => eprintln!("{{\"success\":false,\"error\":\"Serialization failed: {e}\"}}"),
        }
    }
}

impl<T: Serialize> Output<T> {
    pub fn success(data: T) -> Self {
        Self {
            success: true,
            data: Some(data),
            error: None,
            message: None,
        }
    }

    pub fn success_msg(message: impl Into<String>) -> Self {
        Self {
            success: true,
            data: None,
            error: None,
            message: Some(message.into()),
        }
    }

    pub fn error(err: impl Into<String>) -> Self {
        Self {
            success: false,
            data: None,
            error: Some(err.into()),
            message: None,
        }
    }

    pub fn print(&self) {
        match serde_json::to_string_pretty(self) {
            Ok(json) => println!("{json}"),
            Err(e) => eprintln!("{{\"success\":false,\"error\":\"Serialization failed: {e}\"}}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_email_address_display_with_name() {
        let addr = EmailAddress {
            name: Some("John Doe".to_string()),
            email: "john@example.com".to_string(),
        };
        assert_eq!(format!("{}", addr), "John Doe <john@example.com>");
    }

    #[test]
    fn test_email_address_display_without_name() {
        let addr = EmailAddress {
            name: None,
            email: "john@example.com".to_string(),
        };
        assert_eq!(format!("{}", addr), "john@example.com");
    }

    #[test]
    fn test_email_address_display_empty_name() {
        let addr = EmailAddress {
            name: Some("".to_string()),
            email: "john@example.com".to_string(),
        };
        assert_eq!(format!("{}", addr), "john@example.com");
    }

    #[test]
    fn test_email_is_unread() {
        let mut email = Email {
            id: "test".to_string(),
            blob_id: None,
            thread_id: None,
            mailbox_ids: HashMap::new(),
            keywords: HashMap::new(),
            size: 0,
            received_at: None,
            message_id: None,
            in_reply_to: None,
            references: None,
            from: None,
            to: None,
            cc: None,
            bcc: None,
            reply_to: None,
            subject: None,
            sent_at: None,
            preview: None,
            has_attachment: false,
            text_body: None,
            html_body: None,
            attachments: None,
            body_values: None,
        };
        assert!(email.is_unread());
        email.keywords.insert("$seen".to_string(), true);
        assert!(!email.is_unread());
    }

    #[test]
    fn test_email_is_flagged() {
        let mut email = Email {
            id: "test".to_string(),
            blob_id: None,
            thread_id: None,
            mailbox_ids: HashMap::new(),
            keywords: HashMap::new(),
            size: 0,
            received_at: None,
            message_id: None,
            in_reply_to: None,
            references: None,
            from: None,
            to: None,
            cc: None,
            bcc: None,
            reply_to: None,
            subject: None,
            sent_at: None,
            preview: None,
            has_attachment: false,
            text_body: None,
            html_body: None,
            attachments: None,
            body_values: None,
        };
        assert!(!email.is_flagged());
        email.keywords.insert("$flagged".to_string(), true);
        assert!(email.is_flagged());
    }

    #[test]
    fn test_output_success() {
        let output: Output<&str> = Output::success("test data");
        assert!(output.success);
        assert_eq!(output.data, Some("test data"));
        assert!(output.error.is_none());
    }

    #[test]
    fn test_output_error() {
        let output: Output<()> = Output::error("something broke");
        assert!(!output.success);
        assert!(output.data.is_none());
        assert_eq!(output.error, Some("something broke".to_string()));
    }

    #[test]
    fn test_session_deserialize() {
        let json = r#"{
            "capabilities": {},
            "accounts": {},
            "primaryAccounts": {"urn:ietf:params:jmap:mail": "acc1"},
            "username": "test@example.com",
            "apiUrl": "https://api.example.com/jmap",
            "downloadUrl": "https://api.example.com/download",
            "uploadUrl": "https://api.example.com/upload"
        }"#;
        let session: Session = serde_json::from_str(json).unwrap();
        assert_eq!(session.username, "test@example.com");
        assert_eq!(session.primary_account_id(), Some("acc1"));
    }

    #[test]
    fn test_mailbox_deserialize() {
        let json = r#"{
            "id": "mb1",
            "name": "Inbox",
            "role": "inbox",
            "totalEmails": 100,
            "unreadEmails": 5
        }"#;
        let mailbox: Mailbox = serde_json::from_str(json).unwrap();
        assert_eq!(mailbox.id, "mb1");
        assert_eq!(mailbox.name, "Inbox");
        assert_eq!(mailbox.role, Some("inbox".to_string()));
        assert_eq!(mailbox.total_emails, 100);
        assert_eq!(mailbox.unread_emails, 5);
    }

    fn email_with_from(addrs: &[&str]) -> Email {
        let mut e = Email {
            id: "x".into(),
            blob_id: None,
            thread_id: None,
            mailbox_ids: HashMap::new(),
            keywords: HashMap::new(),
            size: 0,
            received_at: None,
            message_id: None,
            in_reply_to: None,
            references: None,
            from: None,
            to: None,
            cc: None,
            bcc: None,
            reply_to: None,
            subject: None,
            sent_at: None,
            preview: None,
            has_attachment: false,
            text_body: None,
            html_body: None,
            attachments: None,
            body_values: None,
        };
        e.from = Some(
            addrs
                .iter()
                .map(|a| EmailAddress {
                    name: None,
                    email: a.to_string(),
                })
                .collect(),
        );
        e
    }

    #[test]
    fn test_email_domain_basic() {
        assert_eq!(email_domain("user@example.com"), Some("example.com"));
        assert_eq!(email_domain("a@b@c.com"), Some("c.com"));
    }

    #[test]
    fn test_email_domain_missing_or_empty() {
        assert_eq!(email_domain("noatsign"), None);
        assert_eq!(email_domain("trailing@"), None);
        assert_eq!(email_domain(""), None);
    }

    #[test]
    fn test_sender_domain_matches_exact() {
        let e = email_with_from(&["a@acme.com"]);
        assert!(sender_domain_matches(&e, "acme.com"));
    }

    #[test]
    fn test_sender_domain_matches_case_insensitive() {
        let e = email_with_from(&["A@Acme.COM"]);
        assert!(sender_domain_matches(&e, "acme.com"));
        assert!(sender_domain_matches(&e, "ACME.com"));
    }

    #[test]
    fn test_sender_domain_subdomain_does_not_match() {
        let e = email_with_from(&["a@mail.acme.com"]);
        assert!(!sender_domain_matches(&e, "acme.com"));
    }

    #[test]
    fn test_sender_domain_no_from() {
        let mut e = email_with_from(&[]);
        e.from = None;
        assert!(!sender_domain_matches(&e, "acme.com"));
        let e2 = email_with_from(&[]);
        assert!(!sender_domain_matches(&e2, "acme.com"));
    }

    #[test]
    fn test_sender_domain_multiple_one_matches() {
        let e = email_with_from(&["a@other.com", "b@acme.com"]);
        assert!(sender_domain_matches(&e, "acme.com"));
    }

    #[test]
    fn test_parse_fields() {
        assert_eq!(parse_fields(None), None);
        assert_eq!(
            parse_fields(Some("id, subject ,from")),
            Some(vec![
                "id".to_string(),
                "subject".to_string(),
                "from".to_string()
            ])
        );
        assert_eq!(parse_fields(Some("")), Some(Vec::<String>::new()));
    }

    #[test]
    fn test_project_fields_keeps_only_requested() {
        let v = serde_json::json!({"id": "1", "subject": "hi", "from": [], "size": 5});
        let out = project_fields(&v, &["id".into(), "subject".into()]);
        assert_eq!(out, serde_json::json!({"id": "1", "subject": "hi"}));
    }

    #[test]
    fn test_project_fields_skips_absent_keys() {
        let v = serde_json::json!({"id": "1"});
        let out = project_fields(&v, &["id".into(), "nope".into()]);
        assert_eq!(out, serde_json::json!({"id": "1"}));
    }

    #[test]
    fn test_project_fields_non_object_passthrough() {
        let v = serde_json::json!([1, 2, 3]);
        let out = project_fields(&v, &["id".into()]);
        assert_eq!(out, v);
    }

    #[test]
    fn test_emails_to_values_projection() {
        let e = email_with_from(&["a@x.com"]);
        let vals = emails_to_values(std::slice::from_ref(&e), Some(&["id".to_string()]));
        assert_eq!(vals.len(), 1);
        assert_eq!(vals[0], serde_json::json!({"id": "x"}));
        // Without projection, full object retains more keys.
        let full = emails_to_values(&[e], None);
        assert!(full[0].as_object().unwrap().contains_key("from"));
    }

    #[test]
    fn test_masked_email_deserialize() {
        let json = r#"{
            "id": "me123",
            "email": "abc123@mask.fastmail.com",
            "state": "enabled",
            "forDomain": "https://example.com",
            "description": "Test site",
            "createdBy": "fastmail-cli"
        }"#;
        let masked: MaskedEmail = serde_json::from_str(json).unwrap();
        assert_eq!(masked.id, "me123");
        assert_eq!(masked.email, "abc123@mask.fastmail.com");
        assert_eq!(masked.state, Some("enabled".to_string()));
        assert_eq!(masked.for_domain, Some("https://example.com".to_string()));
        assert_eq!(masked.description, Some("Test site".to_string()));
        assert_eq!(masked.created_by, Some("fastmail-cli".to_string()));
    }
}
