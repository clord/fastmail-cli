use std::path::{Path, PathBuf};

use crate::jmap::authenticated_client;
use crate::models::{Email, Output};
use crate::util::sanitize_filename;

/// Output format for the `export` command.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, clap::ValueEnum)]
pub enum ExportFormat {
    /// Raw RFC 5322 message saved as a `.eml` file (default).
    #[default]
    Eml,
}

/// Build a stable, filesystem-safe filename for an exported email:
/// `<YYYY-MM-DD>-<subject>-<id>.eml`, omitting the date when unknown and the
/// subject when empty. Path separators in the subject are folded to `-`
/// before sanitizing so the whole subject survives as one path component.
pub(crate) fn eml_filename(email: &Email) -> String {
    let date: String = email
        .received_at
        .as_deref()
        .map(|r| r.chars().take(10).collect())
        .unwrap_or_default();
    let subject = email
        .subject
        .as_deref()
        .unwrap_or("")
        .replace(['/', '\\'], "-");

    let mut stem = String::new();
    for part in [date.as_str(), subject.trim(), email.id.as_str()] {
        if part.is_empty() {
            continue;
        }
        if !stem.is_empty() {
            stem.push('-');
        }
        stem.push_str(part);
    }

    format!("{}.eml", sanitize_filename(&stem, &email.id))
}

/// Resolve where the exported file should be written. `output` is treated as
/// a directory when it ends with a separator or already exists as one;
/// otherwise it is the exact destination file. With no `output` the file
/// lands in the current directory.
pub(crate) fn resolve_output_path(output: Option<&str>, filename: &str) -> PathBuf {
    match output {
        None => PathBuf::from(filename),
        Some(p) if p.ends_with('/') || Path::new(p).is_dir() => Path::new(p).join(filename),
        Some(p) => PathBuf::from(p),
    }
}

/// Export the raw RFC 5322 message (the email's top-level blob) to disk.
pub async fn export_email(
    email_id: &str,
    output: Option<&str>,
    format: ExportFormat,
) -> anyhow::Result<()> {
    // Only one format today; destructure so a future variant forces a match.
    let ExportFormat::Eml = format;

    let client = authenticated_client().await?;
    let email = client.get_email(email_id).await?;
    let blob_id = email.blob_id.as_deref().ok_or_else(|| {
        anyhow::anyhow!("email {email_id} has no blobId; cannot export the raw message")
    })?;
    let bytes = client.download_blob(blob_id).await?;

    let path = resolve_output_path(output, &eml_filename(&email));
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, &bytes)?;

    #[derive(serde::Serialize)]
    struct ExportResponse {
        path: String,
        bytes: usize,
        id: String,
        subject: Option<String>,
    }

    Output::success(ExportResponse {
        path: path.display().to_string(),
        bytes: bytes.len(),
        id: email.id.clone(),
        subject: email.subject.clone(),
    })
    .print();

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn email(id: &str, received_at: Option<&str>, subject: Option<&str>) -> Email {
        let mut obj = serde_json::json!({ "id": id });
        if let Some(r) = received_at {
            obj["receivedAt"] = serde_json::json!(r);
        }
        if let Some(s) = subject {
            obj["subject"] = serde_json::json!(s);
        }
        serde_json::from_value(obj).unwrap()
    }

    #[test]
    fn test_eml_filename_full() {
        let e = email(
            "M123",
            Some("2026-01-15T10:30:00Z"),
            Some("Quarterly report"),
        );
        assert_eq!(eml_filename(&e), "2026-01-15-Quarterly report-M123.eml");
    }

    #[test]
    fn test_eml_filename_subject_with_separators_kept_whole() {
        let e = email(
            "M1",
            Some("2026-01-15T10:30:00Z"),
            Some("Report: alpha/beta"),
        );
        // Without folding, sanitize_filename would keep only "beta".
        assert_eq!(eml_filename(&e), "2026-01-15-Report: alpha-beta-M1.eml");
    }

    #[test]
    fn test_eml_filename_no_subject() {
        let e = email("M9", Some("2025-12-31T23:59:59Z"), None);
        assert_eq!(eml_filename(&e), "2025-12-31-M9.eml");
    }

    #[test]
    fn test_eml_filename_no_date_no_subject() {
        let e = email("Mxyz", None, None);
        assert_eq!(eml_filename(&e), "Mxyz.eml");
    }

    #[test]
    fn test_eml_filename_control_chars_stripped() {
        let e = email("M2", Some("2026-02-02T00:00:00Z"), Some("bad\u{0}name"));
        assert_eq!(eml_filename(&e), "2026-02-02-badname-M2.eml");
    }

    #[test]
    fn test_resolve_output_path_default_cwd() {
        assert_eq!(resolve_output_path(None, "a.eml"), PathBuf::from("a.eml"));
    }

    #[test]
    fn test_resolve_output_path_trailing_slash_is_dir() {
        assert_eq!(
            resolve_output_path(Some("archive/"), "a.eml"),
            PathBuf::from("archive/a.eml")
        );
    }

    #[test]
    fn test_resolve_output_path_existing_dir_is_dir() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().to_str().unwrap().to_string();
        assert_eq!(
            resolve_output_path(Some(&p), "a.eml"),
            dir.path().join("a.eml")
        );
    }

    #[test]
    fn test_resolve_output_path_explicit_file() {
        assert_eq!(
            resolve_output_path(Some("out/message.eml"), "ignored.eml"),
            PathBuf::from("out/message.eml")
        );
    }
}
