mod caldav;
mod carddav;
mod commands;
mod config;
mod error;
mod jmap;
mod mcp;
mod models;
pub mod util;

use clap::{Args, CommandFactory, Parser, Subcommand};
use clap_complete::{Shell, generate};
use models::Output;
use std::io;
use tracing_subscriber::EnvFilter;

/// Build ComposeParams with resolved HTML and loaded attachments.
fn build_compose_params<'a>(
    cc: Option<&'a str>,
    bcc: Option<&'a str>,
    from: Option<&'a str>,
    draft: bool,
    html_body: Option<String>,
    html_file: Option<String>,
    attachment_paths: &[String],
) -> anyhow::Result<jmap::ComposeParams<'a>> {
    let resolved_html = util::resolve_html(html_body, html_file)?;
    let attachments: Vec<jmap::AttachmentData> = attachment_paths
        .iter()
        .map(|p| util::load_attachment(p))
        .collect::<anyhow::Result<Vec<_>>>()?;
    Ok(jmap::ComposeParams {
        cc: cc.map(util::parse_addresses).unwrap_or_default(),
        bcc: bcc.map(util::parse_addresses).unwrap_or_default(),
        from,
        draft,
        html_body: resolved_html,
        attachments,
    })
}

#[derive(Parser)]
#[command(name = "fastmail-cli")]
#[command(version, about = "CLI for Fastmail's JMAP API", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

/// Base search-filter flags shared by every filter-aware command. The keyword
/// presence/absence selectors live in `KeywordSelectArgs` so the `flag`
/// command (whose own `--keyword` action flag would otherwise collide) can
/// flatten the base without them.
#[derive(Args, Debug, Clone)]
struct BaseFilterArgs {
    /// Full-text search (from, to, cc, bcc, subject, body)
    #[arg(short, long)]
    text: Option<String>,

    /// Filter by From header (substring match)
    #[arg(long)]
    from: Option<String>,

    /// Filter by exact sender domain (subdomains excluded)
    #[arg(long)]
    from_domain: Option<String>,

    /// Filter by To header
    #[arg(long)]
    to: Option<String>,

    /// Filter by Cc header
    #[arg(long)]
    cc: Option<String>,

    /// Filter by Bcc header
    #[arg(long)]
    bcc: Option<String>,

    /// Filter by Subject
    #[arg(long)]
    subject: Option<String>,

    /// Filter by body content
    #[arg(long)]
    body: Option<String>,

    /// Filter by mailbox name
    #[arg(short, long)]
    mailbox: Option<String>,

    /// Only emails with attachments
    #[arg(long)]
    has_attachment: bool,

    /// Minimum email size in bytes
    #[arg(long)]
    min_size: Option<u32>,

    /// Maximum email size in bytes
    #[arg(long)]
    max_size: Option<u32>,

    /// Emails received before date (ISO 8601, e.g., 2024-01-01)
    #[arg(long)]
    before: Option<String>,

    /// Emails received on or after date (ISO 8601, e.g., 2024-01-01)
    #[arg(long)]
    after: Option<String>,

    /// Only unread emails
    #[arg(long)]
    unread: bool,

    /// Only flagged/starred emails
    #[arg(long)]
    flagged: bool,
}

/// Keyword presence/absence selectors. Kept separate so `flag` can reuse the
/// base filter without colliding with its own `--keyword` action flag.
#[derive(Args, Debug, Clone, Default)]
struct KeywordSelectArgs {
    /// Require this keyword to be present (repeatable)
    #[arg(long = "keyword", action = clap::ArgAction::Append)]
    keyword: Vec<String>,

    /// Require this keyword to be absent (repeatable)
    #[arg(long = "not-keyword", action = clap::ArgAction::Append)]
    not_keyword: Vec<String>,
}

/// Full reusable search-filter flags, flattened into `search`, `move`, `spam`,
/// and `delete`. Single source of truth converting into
/// `commands::SearchFilter`.
#[derive(Args, Debug, Clone)]
struct FilterArgs {
    #[command(flatten)]
    base: BaseFilterArgs,
    #[command(flatten)]
    keywords: KeywordSelectArgs,
}

impl BaseFilterArgs {
    fn into_filter(self, keywords: KeywordSelectArgs) -> commands::SearchFilter {
        commands::SearchFilter {
            text: self.text,
            from: self.from,
            to: self.to,
            cc: self.cc,
            bcc: self.bcc,
            subject: self.subject,
            body: self.body,
            mailbox: self.mailbox,
            has_attachment: self.has_attachment,
            min_size: self.min_size,
            max_size: self.max_size,
            before: self.before,
            after: self.after,
            unread: self.unread,
            flagged: self.flagged,
            from_domain: self.from_domain,
            keyword: keywords.keyword,
            not_keyword: keywords.not_keyword,
        }
    }
}

impl From<FilterArgs> for commands::SearchFilter {
    fn from(a: FilterArgs) -> Self {
        a.base.into_filter(a.keywords)
    }
}

#[derive(Subcommand)]
enum Commands {
    /// Authenticate with Fastmail API token
    Auth {
        /// API token from Fastmail settings. If omitted, the token is read from
        /// stdin so it doesn't appear in `ps`, shell history, or the environment.
        token: Option<String>,
    },

    /// List resources
    #[command(subcommand)]
    List(ListCommands),

    /// Get a specific email by ID
    Get {
        /// Email ID
        email_id: String,

        /// Output format: json (default, full Email envelope), text (clean
        /// plain text body), or html (raw HTML body). text/html print raw to
        /// stdout for piping.
        #[arg(long, value_enum, default_value_t = commands::GetFormat::Json)]
        format: commands::GetFormat,
    },

    /// Get all emails in a thread/conversation
    Thread {
        /// Email ID (will fetch entire thread this email belongs to)
        email_id: String,

        /// Only output the From of the most recent message in the thread
        #[arg(long)]
        last_sender: bool,

        /// Output format: json (default, full thread) or digest (compact
        /// plain-text summary block, printed raw to stdout)
        #[arg(long, value_enum, default_value_t = commands::ThreadFormat::Json, conflicts_with = "last_sender")]
        format: commands::ThreadFormat,
    },

    /// Search emails with JMAP filters
    Search {
        #[command(flatten)]
        filter: FilterArgs,

        /// Maximum results
        #[arg(short, long, default_value = "50")]
        limit: u32,

        /// Result offset (position) for paging
        #[arg(long, default_value = "0")]
        offset: u32,

        /// Comma-separated top-level fields to keep (e.g. id,subject,from)
        #[arg(long)]
        fields: Option<String>,

        /// Emit one compact JSON object per line (for piping)
        #[arg(long)]
        jsonl: bool,

        /// Incremental sync: return changes since this Email state token
        #[arg(long)]
        since: Option<String>,
    },

    /// Send an email
    Send {
        /// Recipient(s), comma-separated
        #[arg(long)]
        to: String,

        /// Subject line
        #[arg(long)]
        subject: String,

        /// Email body (plain text)
        #[arg(long)]
        body: String,

        /// CC recipient(s), comma-separated
        #[arg(long)]
        cc: Option<String>,

        /// BCC recipient(s), comma-separated
        #[arg(long)]
        bcc: Option<String>,

        /// In-Reply-To message ID (for threading)
        #[arg(long)]
        reply_to: Option<String>,

        /// Send from a specific identity (email address). Use `list identities` to see available.
        #[arg(long)]
        from: Option<String>,

        /// Save as draft instead of sending
        #[arg(long)]
        draft: bool,

        /// HTML body content
        #[arg(long, conflicts_with = "html_file", conflicts_with = "markdown")]
        html_body: Option<String>,

        /// Path to HTML file for email body
        #[arg(long, conflicts_with = "html_body", conflicts_with = "markdown")]
        html_file: Option<String>,

        /// Treat --body as Markdown: render it to HTML for the HTML body while
        /// keeping the Markdown as the plain-text body.
        #[arg(long, conflicts_with = "html_body", conflicts_with = "html_file")]
        markdown: bool,

        /// File attachment (repeatable)
        #[arg(long = "attachment", short = 'a', action = clap::ArgAction::Append)]
        attachments: Vec<String>,
    },

    /// Move emails to a mailbox (by id(s) or by search filter)
    Move {
        /// Email ID(s) to move; omit to select targets via filter flags
        ids: Vec<String>,

        /// Destination mailbox by name (use --to-id for an explicit id)
        #[arg(long = "to-mailbox", visible_alias = "to-name")]
        to_mailbox: Option<String>,

        /// Destination mailbox id
        #[arg(long)]
        to_id: Option<String>,

        #[command(flatten)]
        filter: FilterArgs,

        /// Maximum targets to resolve from a filter
        #[arg(short, long, default_value = "50")]
        limit: u32,

        /// Show would-be-affected emails and action without mutating
        #[arg(long)]
        dry_run: bool,
    },

    /// Mark emails as spam (by id(s) or by search filter)
    Spam {
        /// Email ID(s) to mark; omit to select targets via filter flags
        ids: Vec<String>,

        #[command(flatten)]
        filter: FilterArgs,

        /// Maximum targets to resolve from a filter
        #[arg(short, long, default_value = "50")]
        limit: u32,

        /// Show would-be-affected emails and action without mutating
        #[arg(long)]
        dry_run: bool,

        /// Skip confirmation
        #[arg(short = 'y', long)]
        yes: bool,
    },

    /// Mark email as read or unread
    MarkRead {
        /// Email ID
        email_id: String,

        /// Mark as unread instead of read
        #[arg(long)]
        unread: bool,
    },

    /// Download attachments from an email
    Download {
        /// Email ID
        email_id: String,

        /// Output directory (default: current directory)
        #[arg(short, long)]
        output: Option<String>,

        /// Output format: raw (save files) or json (extract text)
        #[arg(short, long)]
        format: Option<String>,

        /// Max size for images (e.g., 500K, 1M). Images larger than this are resized.
        #[arg(long)]
        max_size: Option<String>,
    },

    /// Reply to an email
    Reply {
        /// Email ID to reply to
        email_id: String,

        /// Reply body (plain text)
        #[arg(long)]
        body: String,

        /// Reply to all recipients
        #[arg(long)]
        all: bool,

        /// Additional CC recipient(s), comma-separated
        #[arg(long)]
        cc: Option<String>,

        /// BCC recipient(s), comma-separated
        #[arg(long)]
        bcc: Option<String>,

        /// Send from a specific identity (email address). Use `list identities` to see available.
        #[arg(long)]
        from: Option<String>,

        /// Save as draft instead of sending
        #[arg(long)]
        draft: bool,

        /// HTML body content
        #[arg(long, conflicts_with = "html_file", conflicts_with = "markdown")]
        html_body: Option<String>,

        /// Path to HTML file for email body
        #[arg(long, conflicts_with = "html_body", conflicts_with = "markdown")]
        html_file: Option<String>,

        /// Treat --body as Markdown: render it to HTML for the HTML body while
        /// keeping the Markdown as the plain-text body.
        #[arg(long, conflicts_with = "html_body", conflicts_with = "html_file")]
        markdown: bool,

        /// File attachment (repeatable)
        #[arg(long = "attachment", short = 'a', action = clap::ArgAction::Append)]
        attachments: Vec<String>,
    },

    /// Forward an email
    Forward {
        /// Email ID to forward
        email_id: String,

        /// Recipient(s), comma-separated
        #[arg(long)]
        to: String,

        /// Message to include before forwarded content
        #[arg(long, default_value = "")]
        body: String,

        /// CC recipient(s), comma-separated
        #[arg(long)]
        cc: Option<String>,

        /// BCC recipient(s), comma-separated
        #[arg(long)]
        bcc: Option<String>,

        /// Send from a specific identity (email address). Use `list identities` to see available.
        #[arg(long)]
        from: Option<String>,

        /// Save as draft instead of sending
        #[arg(long)]
        draft: bool,

        /// HTML body content
        #[arg(long, conflicts_with = "html_file")]
        html_body: Option<String>,

        /// Path to HTML file for email body
        #[arg(long, conflicts_with = "html_body")]
        html_file: Option<String>,

        /// File attachment (repeatable)
        #[arg(long = "attachment", short = 'a', action = clap::ArgAction::Append)]
        attachments: Vec<String>,
    },

    /// Watch the account for changes via JMAP push (Server-Sent Events).
    /// Prints one StateChange JSON object per line; reconnects automatically.
    Watch {
        /// JMAP type(s) to watch, comma-separated (e.g. "Email,Mailbox") or "*"
        #[arg(long, default_value = "Email")]
        types: String,

        /// Server ping interval in seconds (used for dead-connection detection)
        #[arg(long, default_value = "300")]
        ping: u32,

        /// Exit after the first StateChange (for shell loops)
        #[arg(long)]
        once: bool,
    },

    /// Generate shell completions
    Completions {
        /// Shell to generate completions for
        #[arg(value_enum)]
        shell: Shell,
    },

    /// Manage mailboxes (folders)
    #[command(subcommand)]
    Mailbox(MailboxCommands),

    /// Manage masked email addresses
    #[command(subcommand)]
    Masked(MaskedCommands),

    /// Manage contacts via CardDAV
    #[command(subcommand)]
    Contacts(ContactsCommands),

    /// Manage calendars via CalDAV
    #[command(subcommand)]
    Calendar(CalendarCommands),

    /// Manage calendar events via CalDAV
    #[command(subcommand)]
    Event(EventCommands),

    /// Delete emails: move to Trash, or permanently destroy with --hard
    /// (by id(s) or by search filter)
    Delete {
        /// Email ID(s) to delete; omit to select targets via filter flags
        ids: Vec<String>,

        #[command(flatten)]
        filter: FilterArgs,

        /// Maximum targets to resolve from a filter
        #[arg(short, long, default_value = "50")]
        limit: u32,

        /// Permanently destroy instead of moving to Trash (irreversible)
        #[arg(long)]
        hard: bool,

        /// Show would-be-affected emails and action without mutating
        #[arg(long)]
        dry_run: bool,

        /// Skip confirmation
        #[arg(short = 'y', long)]
        yes: bool,
    },

    /// Add or remove keywords on emails (by id(s) or by search filter)
    Flag {
        /// Email ID(s) to flag; omit to select targets via filter flags
        ids: Vec<String>,

        /// Keyword to add (or remove with --remove); repeatable
        #[arg(long = "keyword", action = clap::ArgAction::Append)]
        keywords: Vec<String>,

        /// Remove the listed keywords instead of adding them
        #[arg(long)]
        remove: bool,

        // Note: the base filter is flattened WITHOUT the keyword selectors,
        // since `flag`'s own `--keyword` action flag occupies that name.
        #[command(flatten)]
        filter: BaseFilterArgs,

        /// Maximum targets to resolve from a filter
        #[arg(short, long, default_value = "50")]
        limit: u32,

        /// Show would-be-affected emails and action without mutating
        #[arg(long)]
        dry_run: bool,
    },

    /// Extract hyperlinks (anchor text + URL) from an email, with tracking
    /// redirects stripped
    Links {
        /// Email ID
        email_id: String,
    },

    /// Check whether the latest message in a thread was sent by one of my identities
    Replied {
        /// Email ID (will fetch the thread this email belongs to)
        email_id: String,
    },

    /// Export the raw RFC 5322 message to a file (e.g. for archival)
    Export {
        /// Email ID
        email_id: String,

        /// Destination: a directory (trailing `/` or existing dir) or an
        /// exact file path. Defaults to the current directory.
        #[arg(short, long)]
        output: Option<String>,

        /// Export format (only eml today)
        #[arg(long, value_enum, default_value_t = commands::ExportFormat::Eml)]
        format: commands::ExportFormat,
    },

    /// Run as MCP (Model Context Protocol) server for Claude integration
    Mcp,
}

#[derive(Subcommand)]
enum MailboxCommands {
    /// List mailboxes (folders)
    List,

    /// Create a new mailbox (folder)
    Create {
        /// Mailbox name
        name: String,

        /// Parent mailbox id (omit for a top-level mailbox)
        #[arg(long)]
        parent: Option<String>,
    },

    /// Delete a mailbox by id
    Delete {
        /// Mailbox id
        id: String,

        /// Also delete any emails it contains (otherwise the call fails if non-empty)
        #[arg(long)]
        remove_emails: bool,

        /// Skip confirmation
        #[arg(short = 'y', long)]
        yes: bool,
    },
}

#[derive(Subcommand)]
enum MaskedCommands {
    /// List all masked email addresses
    List,

    /// Create a new masked email address
    Create {
        /// Domain this masked email is for (e.g., https://example.com)
        #[arg(long)]
        domain: Option<String>,

        /// Description for the masked email
        #[arg(long)]
        description: Option<String>,

        /// Custom prefix for the email address (max 64 chars, a-z/0-9/underscore)
        #[arg(long)]
        prefix: Option<String>,
    },

    /// Enable a masked email address
    Enable {
        /// Masked email ID
        id: String,
    },

    /// Disable a masked email address
    Disable {
        /// Masked email ID
        id: String,
    },

    /// Delete a masked email address
    Delete {
        /// Masked email ID
        id: String,

        /// Skip confirmation
        #[arg(short = 'y', long)]
        yes: bool,
    },
}

#[derive(Subcommand)]
enum ListCommands {
    /// List mailboxes (folders)
    Mailboxes,

    /// List emails in a mailbox
    Emails {
        /// Mailbox name (default: INBOX)
        #[arg(short, long, default_value = "INBOX")]
        mailbox: String,

        /// Maximum results
        #[arg(short, long, default_value = "50")]
        limit: u32,

        /// Result offset (position) for paging
        #[arg(long, default_value = "0")]
        offset: u32,

        /// Comma-separated top-level fields to keep (e.g. id,subject,from)
        #[arg(long)]
        fields: Option<String>,

        /// Emit one compact JSON object per line (for piping)
        #[arg(long)]
        jsonl: bool,
    },

    /// List sender identities (for use with --from)
    Identities,
}

#[derive(Subcommand)]
enum ContactsCommands {
    /// List all contacts
    List,

    /// Search contacts by name or email
    Search {
        /// Search query
        query: String,
    },

    /// Check whether an email address belongs to a known contact
    IsKnown {
        /// Email address to look up
        email: String,
    },

    /// Create a new contact
    Create {
        /// Full name
        #[arg(long)]
        name: String,

        /// Email address(es), comma-separated
        #[arg(long)]
        email: Option<String>,

        /// Phone number(s), comma-separated
        #[arg(long)]
        phone: Option<String>,

        /// Organization/company
        #[arg(long)]
        organization: Option<String>,

        /// Job title
        #[arg(long)]
        title: Option<String>,

        /// Notes
        #[arg(long)]
        notes: Option<String>,
    },

    /// Update an existing contact
    Update {
        /// Contact ID
        contact_id: String,

        /// Full name
        #[arg(long)]
        name: Option<String>,

        /// Email address(es), comma-separated (replaces existing)
        #[arg(long)]
        email: Option<String>,

        /// Phone number(s), comma-separated (replaces existing)
        #[arg(long)]
        phone: Option<String>,

        /// Organization/company
        #[arg(long)]
        organization: Option<String>,

        /// Job title
        #[arg(long)]
        title: Option<String>,

        /// Notes
        #[arg(long)]
        notes: Option<String>,
    },

    /// Delete a contact
    Delete {
        /// Contact ID
        contact_id: String,

        /// Skip confirmation
        #[arg(short = 'y', long)]
        yes: bool,
    },
}

#[derive(Subcommand)]
enum CalendarCommands {
    /// List calendars
    List,
}

#[derive(Subcommand)]
enum EventCommands {
    /// Create a calendar event
    Create {
        /// Calendar name (case-insensitive; see `calendar list`)
        #[arg(long)]
        calendar: String,

        /// Event title (SUMMARY)
        #[arg(long)]
        title: String,

        /// Start: YYYY-MM-DDTHH:MM[:SS][Z] (no Z = floating local time), or
        /// YYYY-MM-DD with --all-day
        #[arg(long)]
        start: String,

        /// End, same format as --start (exclusive, per iCalendar)
        #[arg(long, conflicts_with = "duration")]
        end: Option<String>,

        /// Duration, e.g. 1h, 90m, 1h30m, 2d (default: 1h timed, 1 day all-day)
        #[arg(long)]
        duration: Option<String>,

        /// All-day event (--start/--end are plain dates)
        #[arg(long)]
        all_day: bool,

        /// Location
        #[arg(long)]
        location: Option<String>,

        /// Description/notes
        #[arg(long)]
        notes: Option<String>,

        /// Recurrence rule, e.g. FREQ=WEEKLY;BYDAY=MO
        #[arg(long)]
        rrule: Option<String>,
    },
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_target(false)
        .init();

    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Auth { token } => {
            let resolved = match token {
                Some(t) => Ok(t),
                None => commands::read_token_from_stdin(),
            };
            match resolved {
                Ok(t) => commands::auth(&t).await,
                Err(e) => Err(e),
            }
        }

        Commands::List(cmd) => match cmd {
            ListCommands::Mailboxes => commands::list_mailboxes().await,
            ListCommands::Emails {
                mailbox,
                limit,
                offset,
                fields,
                jsonl,
            } => commands::list_emails(&mailbox, limit, offset, fields, jsonl).await,
            ListCommands::Identities => commands::list_identities().await,
        },

        Commands::Get { email_id, format } => commands::get_email_fmt(&email_id, format).await,

        Commands::Thread {
            email_id,
            last_sender,
            format,
        } => {
            if last_sender {
                commands::thread_last_sender(&email_id).await
            } else {
                commands::get_thread_fmt(&email_id, format).await
            }
        }

        Commands::Search {
            filter,
            limit,
            offset,
            fields,
            jsonl,
            since,
        } => commands::search(filter.into(), limit, offset, fields, jsonl, since).await,

        Commands::Send {
            to,
            subject,
            body,
            cc,
            bcc,
            reply_to,
            from,
            draft,
            html_body,
            html_file,
            markdown,
            attachments,
        } => {
            async {
                // When --markdown is set, render the Markdown --body to HTML and
                // use it as the HTML body; the Markdown stays as the text body.
                let html_body = if markdown {
                    Some(util::markdown_to_html(&body))
                } else {
                    html_body
                };
                let params = build_compose_params(
                    cc.as_deref(),
                    bcc.as_deref(),
                    from.as_deref(),
                    draft,
                    html_body,
                    html_file,
                    &attachments,
                )?;
                commands::send(&to, &subject, &body, reply_to.as_deref(), params).await
            }
            .await
        }

        Commands::Move {
            ids,
            to_mailbox,
            to_id,
            filter,
            limit,
            dry_run,
        } => commands::move_bulk(ids, filter.into(), to_mailbox, to_id, limit, dry_run).await,

        Commands::Spam {
            ids,
            filter,
            limit,
            dry_run,
            yes,
        } => {
            if !dry_run && !yes {
                eprintln!("Mark email(s) as spam? Use -y to confirm (or --dry-run to preview).");
                std::process::exit(1);
            }
            commands::spam_bulk(ids, filter.into(), limit, dry_run).await
        }

        Commands::MarkRead { email_id, unread } => commands::mark_read(&email_id, !unread).await,

        Commands::Download {
            email_id,
            output,
            format,
            max_size,
        } => {
            commands::download_attachment(
                &email_id,
                output.as_deref(),
                format.as_deref(),
                max_size.as_deref(),
            )
            .await
        }

        Commands::Reply {
            email_id,
            body,
            all,
            cc,
            bcc,
            from,
            draft,
            html_body,
            html_file,
            markdown,
            attachments,
        } => {
            async {
                // When --markdown is set, render the Markdown --body to HTML and
                // use it as the HTML body; the Markdown stays as the text body.
                let html_body = if markdown {
                    Some(util::markdown_to_html(&body))
                } else {
                    html_body
                };
                let params = build_compose_params(
                    cc.as_deref(),
                    bcc.as_deref(),
                    from.as_deref(),
                    draft,
                    html_body,
                    html_file,
                    &attachments,
                )?;
                commands::reply(&email_id, &body, all, params).await
            }
            .await
        }

        Commands::Watch { types, ping, once } => commands::watch(&types, ping, once).await,

        Commands::Forward {
            email_id,
            to,
            body,
            cc,
            bcc,
            from,
            draft,
            html_body,
            html_file,
            attachments,
        } => {
            async {
                let params = build_compose_params(
                    cc.as_deref(),
                    bcc.as_deref(),
                    from.as_deref(),
                    draft,
                    html_body,
                    html_file,
                    &attachments,
                )?;
                commands::forward(&email_id, &to, &body, params).await
            }
            .await
        }

        Commands::Completions { shell } => {
            generate(
                shell,
                &mut Cli::command(),
                "fastmail-cli",
                &mut io::stdout(),
            );
            return;
        }

        Commands::Mailbox(cmd) => match cmd {
            MailboxCommands::List => commands::list_mailboxes().await,
            MailboxCommands::Create { name, parent } => {
                commands::create_mailbox(&name, parent.as_deref()).await
            }
            MailboxCommands::Delete {
                id,
                remove_emails,
                yes,
            } => {
                if !yes {
                    eprintln!("Delete mailbox {}? Use -y to confirm.", id);
                    std::process::exit(1);
                }
                commands::delete_mailbox(&id, remove_emails).await
            }
        },

        Commands::Masked(cmd) => match cmd {
            MaskedCommands::List => commands::list_masked_emails().await,
            MaskedCommands::Create {
                domain,
                description,
                prefix,
            } => {
                commands::create_masked_email(
                    domain.as_deref(),
                    description.as_deref(),
                    prefix.as_deref(),
                )
                .await
            }
            MaskedCommands::Enable { id } => commands::enable_masked_email(&id).await,
            MaskedCommands::Disable { id } => commands::disable_masked_email(&id).await,
            MaskedCommands::Delete { id, yes } => {
                if !yes {
                    eprintln!("Delete masked email {}? Use -y to confirm.", id);
                    std::process::exit(1);
                }
                commands::delete_masked_email(&id).await
            }
        },

        Commands::Contacts(cmd) => match cmd {
            ContactsCommands::List => commands::list_contacts().await,
            ContactsCommands::Search { query } => commands::search_contacts(&query).await,
            ContactsCommands::IsKnown { email } => commands::contacts_is_known(&email).await,
            ContactsCommands::Create {
                name,
                email,
                phone,
                organization,
                title,
                notes,
            } => {
                commands::create_contact(
                    &name,
                    email.as_deref(),
                    phone.as_deref(),
                    organization.as_deref(),
                    title.as_deref(),
                    notes.as_deref(),
                )
                .await
            }
            ContactsCommands::Update {
                contact_id,
                name,
                email,
                phone,
                organization,
                title,
                notes,
            } => {
                commands::update_contact(
                    &contact_id,
                    name.as_deref(),
                    email.as_deref(),
                    phone.as_deref(),
                    organization.as_deref(),
                    title.as_deref(),
                    notes.as_deref(),
                )
                .await
            }
            ContactsCommands::Delete { contact_id, yes } => {
                if !yes {
                    eprintln!("Delete contact {}? Use -y to confirm.", contact_id);
                    std::process::exit(1);
                }
                commands::delete_contact(&contact_id).await
            }
        },

        Commands::Calendar(cmd) => match cmd {
            CalendarCommands::List => commands::list_calendars().await,
        },

        Commands::Event(cmd) => match cmd {
            EventCommands::Create {
                calendar,
                title,
                start,
                end,
                duration,
                all_day,
                location,
                notes,
                rrule,
            } => {
                commands::create_event(
                    &calendar,
                    &title,
                    &start,
                    end.as_deref(),
                    duration.as_deref(),
                    all_day,
                    location.as_deref(),
                    notes.as_deref(),
                    rrule.as_deref(),
                )
                .await
            }
        },

        Commands::Delete {
            ids,
            filter,
            limit,
            hard,
            dry_run,
            yes,
        } => {
            if !dry_run && !yes {
                eprintln!(
                    "Delete email(s)? Use -y to confirm (or --dry-run to preview).{}",
                    if hard {
                        " (--hard is irreversible)"
                    } else {
                        ""
                    }
                );
                std::process::exit(1);
            }
            commands::delete_bulk(ids, filter.into(), limit, hard, dry_run).await
        }

        Commands::Flag {
            ids,
            keywords,
            remove,
            filter,
            limit,
            dry_run,
        } => {
            let search_filter = filter.into_filter(KeywordSelectArgs::default());
            commands::flag_bulk(ids, search_filter, keywords, remove, limit, dry_run).await
        }

        Commands::Links { email_id } => commands::extract_email_links(&email_id).await,

        Commands::Replied { email_id } => commands::replied(&email_id).await,

        Commands::Export {
            email_id,
            output,
            format,
        } => commands::export_email(&email_id, output.as_deref(), format).await,

        Commands::Mcp => mcp::run_server().await,
    };

    if let Err(e) = result {
        Output::<()>::error(e.to_string()).print();
        std::process::exit(1);
    }
}
