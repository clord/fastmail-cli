use crate::jmap::authenticated_client;
use crate::models::Output;
use crate::util::html_to_text;

/// Output format for the `get` command.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, clap::ValueEnum)]
pub enum GetFormat {
    /// Full Email object wrapped in the JSON Output envelope (default).
    #[default]
    Json,
    /// Clean plain text of the email body, printed raw to stdout.
    Text,
    /// Raw HTML body, printed raw to stdout.
    Html,
}

pub async fn get_email_fmt(email_id: &str, format: GetFormat) -> anyhow::Result<()> {
    let client = authenticated_client().await?;
    let email = client.get_email(email_id).await?;

    match format {
        GetFormat::Json => {
            Output::success(email).print();
        }
        GetFormat::Text => {
            // Prefer the existing plain-text body; otherwise convert HTML.
            let text = if let Some(text) = email.text_content() {
                text.to_string()
            } else if let Some(html) = email.html_content() {
                html_to_text(html)
            } else {
                String::new()
            };
            println!("{text}");
        }
        GetFormat::Html => {
            let html = email.html_content().unwrap_or("");
            println!("{html}");
        }
    }

    Ok(())
}
