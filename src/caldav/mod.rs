//! CalDAV client for Fastmail calendars
//!
//! Mirrors the CardDAV module: CalDAV is the same WebDAV-over-HTTP with
//! app-password Basic auth, just different URLs, XML namespace, and an
//! iCalendar payload instead of vCard.

use percent_encoding::utf8_percent_encode;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tracing::{debug, instrument};

use crate::carddav::PATH_SEGMENT;
use crate::error::{Error, Result};

const CALDAV_BASE: &str = "https://caldav.fastmail.com";

/// Calendar collection info
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Calendar {
    pub href: String,
    pub name: String,
}

/// Fields for creating an event. `start` is `YYYY-MM-DD` for all-day events,
/// otherwise `YYYY-MM-DDTHH:MM[:SS][Z]` (no `Z` = floating local time).
#[derive(Debug, Clone, Default)]
pub struct EventFields<'a> {
    pub title: &'a str,
    pub start: &'a str,
    /// Exclusive end (iCal DTEND semantics), same format as `start`.
    pub end: Option<&'a str>,
    /// Duration like `1h`, `90m`, `1h30m`, `2d`. Defaults: 1h timed, 1 day all-day.
    pub duration: Option<&'a str>,
    pub all_day: bool,
    pub location: Option<&'a str>,
    pub notes: Option<&'a str>,
    /// Raw RRULE value, e.g. `FREQ=WEEKLY;BYDAY=MO`.
    pub rrule: Option<&'a str>,
}

/// What `create_event` returns for JSON output.
#[derive(Debug, Clone, Serialize)]
pub struct CreatedEvent {
    pub uid: String,
    pub href: String,
    pub calendar: String,
    pub title: String,
    pub start: String,
    pub all_day: bool,
}

/// CalDAV client
pub struct CalDavClient {
    client: Client,
    username: String,
    app_password: String,
}

impl CalDavClient {
    pub fn new(username: String, app_password: String) -> Self {
        Self {
            client: Client::new(),
            username,
            app_password,
        }
    }

    /// Discover calendars for the user
    #[instrument(skip(self))]
    pub async fn list_calendars(&self) -> Result<Vec<Calendar>> {
        let encoded_user = utf8_percent_encode(&self.username, PATH_SEGMENT);
        let url = format!("{}/dav/calendars/user/{}/", CALDAV_BASE, encoded_user);

        let body = r#"<?xml version="1.0" encoding="utf-8"?>
<d:propfind xmlns:d="DAV:" xmlns:cal="urn:ietf:params:xml:ns:caldav">
  <d:prop>
    <d:displayname/>
    <d:resourcetype/>
  </d:prop>
</d:propfind>"#;

        let response = self
            .client
            .request(reqwest::Method::from_bytes(b"PROPFIND").unwrap(), &url)
            .basic_auth(&self.username, Some(&self.app_password))
            .header("Content-Type", "application/xml")
            .header("Depth", "1")
            .body(body)
            .send()
            .await?;

        let status = response.status();
        let text: String = response.text().await?;

        debug!(status = %status, "PROPFIND response");

        if !status.is_success() && status.as_u16() != 207 {
            return Err(Error::Server(format!(
                "CalDAV PROPFIND failed: {} - {}",
                status, text
            )));
        }

        parse_calendars_response(&text, &self.username)
    }

    /// Resolve a calendar by display name (case-insensitive exact match).
    pub async fn find_calendar(&self, name: &str) -> Result<Calendar> {
        let calendars = self.list_calendars().await?;
        match_calendar(&calendars, name).cloned().ok_or_else(|| {
            let available: Vec<&str> = calendars.iter().map(|c| c.name.as_str()).collect();
            Error::Server(format!(
                "Calendar not found: {name} (available: {})",
                available.join(", ")
            ))
        })
    }

    /// Create an event in the given calendar via PUT of a minimal
    /// VCALENDAR/VEVENT. `If-None-Match: *` guarantees we never overwrite.
    #[instrument(skip(self, fields))]
    pub async fn create_event(
        &self,
        calendar: &Calendar,
        fields: &EventFields<'_>,
    ) -> Result<CreatedEvent> {
        let uid = uuid::Uuid::new_v4().to_string();
        let dtstamp = dtstamp_now();
        let ics = build_event_ics(&uid, &dtstamp, fields)?;

        let url = format!("{}{}{}.ics", CALDAV_BASE, calendar.href, uid);
        debug!(url = %url, "Creating event");

        let response = self
            .client
            .put(&url)
            .basic_auth(&self.username, Some(&self.app_password))
            .header("Content-Type", "text/calendar; charset=utf-8")
            .header("If-None-Match", "*")
            .body(ics)
            .send()
            .await?;

        let status = response.status();
        let text: String = response.text().await?;

        if !status.is_success() && status.as_u16() != 201 && status.as_u16() != 204 {
            return Err(Error::Server(format!(
                "CalDAV PUT failed: {} - {}",
                status, text
            )));
        }

        Ok(CreatedEvent {
            href: format!("{}{}.ics", calendar.href, uid),
            uid,
            calendar: calendar.name.clone(),
            title: fields.title.to_string(),
            start: fields.start.to_string(),
            all_day: fields.all_day,
        })
    }
}

/// Parse a PROPFIND multistatus response, keeping collections whose
/// resourcetype contains `caldav:calendar` (and skipping the user's parent
/// collection itself).
fn parse_calendars_response(xml: &str, username: &str) -> Result<Vec<Calendar>> {
    let doc = roxmltree::Document::parse(xml)
        .map_err(|e| Error::Server(format!("Failed to parse XML: {e}")))?;

    let dav_ns = "DAV:";
    let caldav_ns = "urn:ietf:params:xml:ns:caldav";
    let mut calendars = Vec::new();

    for response in doc
        .descendants()
        .filter(|n| n.has_tag_name((dav_ns, "response")))
    {
        let href = response
            .descendants()
            .find(|n| n.has_tag_name((dav_ns, "href")))
            .and_then(|n| n.text())
            .unwrap_or_default();

        let is_calendar = response
            .descendants()
            .any(|n| n.has_tag_name((caldav_ns, "calendar")));

        if is_calendar && !href.is_empty() {
            let displayname = response
                .descendants()
                .find(|n| n.has_tag_name((dav_ns, "displayname")))
                .and_then(|n| n.text());

            let name = displayname.map(|s| s.to_string()).unwrap_or_else(|| {
                href.split('/')
                    .rfind(|s| !s.is_empty())
                    .unwrap_or("Unknown")
                    .to_string()
            });

            // Skip the parent collection itself
            if !href.ends_with(&format!("{}/", username)) {
                calendars.push(Calendar {
                    href: href.to_string(),
                    name,
                });
            }
        }
    }

    Ok(calendars)
}

/// Case-insensitive exact match on calendar display name.
fn match_calendar<'a>(calendars: &'a [Calendar], name: &str) -> Option<&'a Calendar> {
    calendars.iter().find(|c| c.name.eq_ignore_ascii_case(name))
}

// ---------------------------------------------------------------------------
// iCalendar construction (pure, unit-tested)
// ---------------------------------------------------------------------------

/// Escape a TEXT value per RFC 5545 §3.3.11.
fn ical_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            ';' => out.push_str("\\;"),
            ',' => out.push_str("\\,"),
            '\n' => out.push_str("\\n"),
            '\r' => {}
            _ => out.push(c),
        }
    }
    out
}

/// Fold a content line at 75 octets (RFC 5545 §3.1), continuing with
/// CRLF + space. Splits only on char boundaries.
fn fold_ical_line(line: &str) -> String {
    const LIMIT: usize = 75;
    if line.len() <= LIMIT {
        return line.to_string();
    }
    let mut out = String::with_capacity(line.len() + line.len() / LIMIT * 3);
    let mut budget = LIMIT;
    for c in line.chars() {
        let w = c.len_utf8();
        if w > budget {
            out.push_str("\r\n ");
            budget = LIMIT - 1; // continuation lines start with one space
        }
        out.push(c);
        budget -= w;
    }
    out
}

/// Normalize a date input (`YYYY-MM-DD` or `YYYYMMDD`) to iCal `YYYYMMDD`.
fn parse_ical_date(input: &str) -> Result<String> {
    let compact: String = input.chars().filter(|c| *c != '-').collect();
    if compact.len() == 8 && compact.chars().all(|c| c.is_ascii_digit()) {
        Ok(compact)
    } else {
        Err(Error::Config(format!(
            "Invalid date '{input}': expected YYYY-MM-DD"
        )))
    }
}

/// Normalize a datetime input (`YYYY-MM-DDTHH:MM[:SS][Z]`, separators
/// optional) to iCal `YYYYMMDDTHHMMSS[Z]`.
fn parse_ical_datetime(input: &str) -> Result<String> {
    let z = input.ends_with('Z') || input.ends_with('z');
    let body = if z { &input[..input.len() - 1] } else { input };

    let Some((date_part, time_part)) = body.split_once(['T', 't']) else {
        return Err(Error::Config(format!(
            "Invalid datetime '{input}': expected YYYY-MM-DDTHH:MM[:SS] (or a plain date with --all-day)"
        )));
    };

    let date = parse_ical_date(date_part)?;
    let time: String = time_part.chars().filter(|c| *c != ':').collect();
    let time = match time.len() {
        4 => format!("{time}00"),
        6 => time,
        _ => {
            return Err(Error::Config(format!(
                "Invalid time in '{input}': expected HH:MM or HH:MM:SS"
            )));
        }
    };
    if !time.chars().all(|c| c.is_ascii_digit()) {
        return Err(Error::Config(format!("Invalid time in '{input}'")));
    }

    Ok(format!("{date}T{time}{}", if z { "Z" } else { "" }))
}

/// Parse a human duration (`1h`, `90m`, `1h30m`, `2d`, `45s`) into an iCal
/// DURATION value (`PT1H`, `PT90M`, `PT1H30M`, `P2D`, `PT45S`).
fn parse_duration(input: &str) -> Result<String> {
    let mut days = 0u64;
    let mut hours = 0u64;
    let mut mins = 0u64;
    let mut secs = 0u64;
    let mut num = String::new();
    let mut saw_unit = false;

    for c in input.trim().chars() {
        if c.is_ascii_digit() {
            num.push(c);
            continue;
        }
        let n: u64 = num
            .parse()
            .map_err(|_| Error::Config(format!("Invalid duration '{input}'")))?;
        num.clear();
        saw_unit = true;
        match c.to_ascii_lowercase() {
            'd' => days += n,
            'h' => hours += n,
            'm' => mins += n,
            's' => secs += n,
            _ => {
                return Err(Error::Config(format!(
                    "Invalid duration unit '{c}' in '{input}' (use d/h/m/s)"
                )));
            }
        }
    }
    if !num.is_empty() || !saw_unit {
        return Err(Error::Config(format!(
            "Invalid duration '{input}': expected forms like 1h, 90m, 1h30m, 2d"
        )));
    }

    let mut out = String::from("P");
    if days > 0 {
        out.push_str(&format!("{days}D"));
    }
    if hours > 0 || mins > 0 || secs > 0 {
        out.push('T');
        if hours > 0 {
            out.push_str(&format!("{hours}H"));
        }
        if mins > 0 {
            out.push_str(&format!("{mins}M"));
        }
        if secs > 0 {
            out.push_str(&format!("{secs}S"));
        }
    }
    if out == "P" {
        return Err(Error::Config(format!("Duration '{input}' is zero")));
    }
    Ok(out)
}

/// Days-since-epoch → (year, month, day). Howard Hinnant's civil_from_days.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// (year, month, day) → days since 1970-01-01. Inverse of `civil_from_days`.
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u64;
    let mp = if m > 2 { m - 3 } else { m + 9 } as u64;
    let doy = (153 * mp + 2) / 5 + d as u64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe as i64 - 719_468
}

/// Format unix seconds as an iCal UTC timestamp `YYYYMMDDTHHMMSSZ`.
fn format_utc_timestamp(unix_secs: u64) -> String {
    let days = (unix_secs / 86_400) as i64;
    let rem = unix_secs % 86_400;
    let (y, m, d) = civil_from_days(days);
    format!(
        "{:04}{:02}{:02}T{:02}{:02}{:02}Z",
        y,
        m,
        d,
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

/// Current time as an iCal DTSTAMP value.
fn dtstamp_now() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    format_utc_timestamp(now.as_secs())
}

/// The day after an iCal `YYYYMMDD` date (for default all-day DTEND, which
/// is exclusive per RFC 5545).
fn next_day(yyyymmdd: &str) -> Result<String> {
    let y: i64 = yyyymmdd[0..4]
        .parse()
        .map_err(|_| Error::Config(format!("Invalid date '{yyyymmdd}'")))?;
    let m: u32 = yyyymmdd[4..6]
        .parse()
        .map_err(|_| Error::Config(format!("Invalid date '{yyyymmdd}'")))?;
    let d: u32 = yyyymmdd[6..8]
        .parse()
        .map_err(|_| Error::Config(format!("Invalid date '{yyyymmdd}'")))?;
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return Err(Error::Config(format!("Invalid date '{yyyymmdd}'")));
    }
    let (ny, nm, nd) = civil_from_days(days_from_civil(y, m, d) + 1);
    Ok(format!("{ny:04}{nm:02}{nd:02}"))
}

/// Build a minimal VCALENDAR/VEVENT. `uid` and `dtstamp` are injected so the
/// output is deterministic and testable. CRLF line endings, folded at 75.
pub(crate) fn build_event_ics(
    uid: &str,
    dtstamp: &str,
    fields: &EventFields<'_>,
) -> Result<String> {
    if fields.title.trim().is_empty() {
        return Err(Error::Config("Event title must not be empty".into()));
    }
    if fields.end.is_some() && fields.duration.is_some() {
        return Err(Error::Config(
            "Provide --end or --duration, not both".into(),
        ));
    }
    if let Some(rrule) = fields.rrule
        && !rrule.to_ascii_uppercase().starts_with("FREQ=")
    {
        return Err(Error::Config(format!(
            "Invalid RRULE '{rrule}': must start with FREQ="
        )));
    }

    let mut lines: Vec<String> = vec![
        "BEGIN:VCALENDAR".into(),
        "VERSION:2.0".into(),
        "PRODID:-//fastmail-cli//EN".into(),
        "BEGIN:VEVENT".into(),
        format!("UID:{uid}"),
        format!("DTSTAMP:{dtstamp}"),
    ];

    if fields.all_day {
        if fields.start.contains(['T', 't']) {
            return Err(Error::Config(
                "--all-day events take a plain date (YYYY-MM-DD), not a time".into(),
            ));
        }
        let start = parse_ical_date(fields.start)?;
        lines.push(format!("DTSTART;VALUE=DATE:{start}"));
        match (fields.end, fields.duration) {
            (Some(end), _) => {
                lines.push(format!("DTEND;VALUE=DATE:{}", parse_ical_date(end)?));
            }
            (None, Some(dur)) => {
                let d = parse_duration(dur)?;
                if d.contains('T') {
                    return Err(Error::Config(format!(
                        "All-day duration must be whole days (e.g. 2d), got '{dur}'"
                    )));
                }
                lines.push(format!("DURATION:{d}"));
            }
            (None, None) => {
                lines.push(format!("DTEND;VALUE=DATE:{}", next_day(&start)?));
            }
        }
    } else {
        let start = parse_ical_datetime(fields.start)?;
        lines.push(format!("DTSTART:{start}"));
        match (fields.end, fields.duration) {
            (Some(end), _) => lines.push(format!("DTEND:{}", parse_ical_datetime(end)?)),
            (None, Some(dur)) => lines.push(format!("DURATION:{}", parse_duration(dur)?)),
            // Default: one hour.
            (None, None) => lines.push("DURATION:PT1H".into()),
        }
    }

    lines.push(format!("SUMMARY:{}", ical_escape(fields.title)));
    if let Some(location) = fields.location {
        lines.push(format!("LOCATION:{}", ical_escape(location)));
    }
    if let Some(notes) = fields.notes {
        lines.push(format!("DESCRIPTION:{}", ical_escape(notes)));
    }
    if let Some(rrule) = fields.rrule {
        lines.push(format!("RRULE:{}", rrule.to_ascii_uppercase()));
    }
    lines.push("END:VEVENT".into());
    lines.push("END:VCALENDAR".into());

    let folded: Vec<String> = lines.iter().map(|l| fold_ical_line(l)).collect();
    Ok(folded.join("\r\n") + "\r\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- escaping & folding --------------------------------------------------

    #[test]
    fn test_ical_escape() {
        assert_eq!(ical_escape("plain"), "plain");
        assert_eq!(ical_escape("a;b,c\\d"), "a\\;b\\,c\\\\d");
        assert_eq!(ical_escape("line1\nline2"), "line1\\nline2");
        assert_eq!(ical_escape("crlf\r\nkept"), "crlf\\nkept");
    }

    #[test]
    fn test_fold_short_line_unchanged() {
        assert_eq!(fold_ical_line("SUMMARY:short"), "SUMMARY:short");
    }

    #[test]
    fn test_fold_long_line() {
        let line = format!("SUMMARY:{}", "x".repeat(100));
        let folded = fold_ical_line(&line);
        for part in folded.split("\r\n") {
            assert!(part.len() <= 75, "segment too long: {}", part.len());
        }
        // Unfolding (remove CRLF + single space) restores the original.
        assert_eq!(folded.replace("\r\n ", ""), line);
    }

    #[test]
    fn test_fold_multibyte_on_char_boundary() {
        let line = format!("SUMMARY:{}", "é".repeat(100));
        let folded = fold_ical_line(&line);
        for part in folded.split("\r\n") {
            assert!(part.len() <= 75);
        }
        assert_eq!(folded.replace("\r\n ", ""), line);
    }

    // -- date/time parsing ---------------------------------------------------

    #[test]
    fn test_parse_ical_date() {
        assert_eq!(parse_ical_date("2026-06-20").unwrap(), "20260620");
        assert_eq!(parse_ical_date("20260620").unwrap(), "20260620");
        assert!(parse_ical_date("2026-06").is_err());
        assert!(parse_ical_date("not-a-date").is_err());
    }

    #[test]
    fn test_parse_ical_datetime() {
        assert_eq!(
            parse_ical_datetime("2026-06-20T14:00:00").unwrap(),
            "20260620T140000"
        );
        assert_eq!(
            parse_ical_datetime("2026-06-20T14:00").unwrap(),
            "20260620T140000"
        );
        assert_eq!(
            parse_ical_datetime("2026-06-20T14:00:00Z").unwrap(),
            "20260620T140000Z"
        );
        assert_eq!(
            parse_ical_datetime("20260620T140000").unwrap(),
            "20260620T140000"
        );
        assert!(parse_ical_datetime("2026-06-20").is_err());
        assert!(parse_ical_datetime("2026-06-20T9").is_err());
    }

    // -- durations -----------------------------------------------------------

    #[test]
    fn test_parse_duration() {
        assert_eq!(parse_duration("1h").unwrap(), "PT1H");
        assert_eq!(parse_duration("90m").unwrap(), "PT90M");
        assert_eq!(parse_duration("1h30m").unwrap(), "PT1H30M");
        assert_eq!(parse_duration("2d").unwrap(), "P2D");
        assert_eq!(parse_duration("1d2h").unwrap(), "P1DT2H");
        assert_eq!(parse_duration("45s").unwrap(), "PT45S");
        assert!(parse_duration("").is_err());
        assert!(parse_duration("90").is_err());
        assert!(parse_duration("1x").is_err());
        assert!(parse_duration("0m").is_err());
    }

    // -- civil date math -----------------------------------------------------

    #[test]
    fn test_civil_epoch() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(days_from_civil(1970, 1, 1), 0);
    }

    #[test]
    fn test_civil_round_trip() {
        for z in [-1000i64, -1, 0, 1, 10_000, 20_000, 60_000] {
            let (y, m, d) = civil_from_days(z);
            assert_eq!(days_from_civil(y, m, d), z);
        }
    }

    #[test]
    fn test_format_utc_timestamp() {
        assert_eq!(format_utc_timestamp(0), "19700101T000000Z");
        assert_eq!(format_utc_timestamp(86_399), "19700101T235959Z");
        // 2000-02-29 00:00:00 UTC (leap day)
        assert_eq!(format_utc_timestamp(951_782_400), "20000229T000000Z");
    }

    #[test]
    fn test_next_day() {
        assert_eq!(next_day("20260620").unwrap(), "20260621");
        assert_eq!(next_day("20261231").unwrap(), "20270101");
        assert_eq!(next_day("20240228").unwrap(), "20240229"); // leap year
        assert_eq!(next_day("20250228").unwrap(), "20250301");
    }

    // -- VEVENT assembly -----------------------------------------------------

    fn fields<'a>() -> EventFields<'a> {
        EventFields {
            title: "Dentist",
            start: "2026-06-20T14:00:00",
            ..Default::default()
        }
    }

    #[test]
    fn test_build_timed_event_default_duration() {
        let ics = build_event_ics("UID1", "20260601T000000Z", &fields()).unwrap();
        assert!(ics.contains("BEGIN:VCALENDAR\r\n"));
        assert!(ics.contains("UID:UID1\r\n"));
        assert!(ics.contains("DTSTAMP:20260601T000000Z\r\n"));
        assert!(ics.contains("DTSTART:20260620T140000\r\n"));
        assert!(ics.contains("DURATION:PT1H\r\n"));
        assert!(ics.contains("SUMMARY:Dentist\r\n"));
        assert!(ics.ends_with("END:VCALENDAR\r\n"));
    }

    #[test]
    fn test_build_timed_event_with_duration_location_notes() {
        let f = EventFields {
            duration: Some("1h30m"),
            location: Some("12 Main St; Suite 4"),
            notes: Some("bring x-rays\nand insurance card"),
            ..fields()
        };
        let ics = build_event_ics("UID1", "20260601T000000Z", &f).unwrap();
        assert!(ics.contains("DURATION:PT1H30M\r\n"));
        assert!(ics.contains("LOCATION:12 Main St\\; Suite 4\r\n"));
        assert!(ics.contains("DESCRIPTION:bring x-rays\\nand insurance card\r\n"));
    }

    #[test]
    fn test_build_timed_event_with_end() {
        let f = EventFields {
            end: Some("2026-06-20T15:30:00"),
            ..fields()
        };
        let ics = build_event_ics("UID1", "20260601T000000Z", &f).unwrap();
        assert!(ics.contains("DTEND:20260620T153000\r\n"));
        assert!(!ics.contains("DURATION:"));
    }

    #[test]
    fn test_build_all_day_event_defaults_one_day() {
        let f = EventFields {
            start: "2026-06-20",
            all_day: true,
            ..fields()
        };
        let ics = build_event_ics("UID1", "20260601T000000Z", &f).unwrap();
        assert!(ics.contains("DTSTART;VALUE=DATE:20260620\r\n"));
        assert!(ics.contains("DTEND;VALUE=DATE:20260621\r\n"));
    }

    #[test]
    fn test_build_all_day_rejects_time() {
        let f = EventFields {
            start: "2026-06-20T14:00:00",
            all_day: true,
            ..fields()
        };
        assert!(build_event_ics("UID1", "20260601T000000Z", &f).is_err());
    }

    #[test]
    fn test_build_all_day_rejects_sub_day_duration() {
        let f = EventFields {
            start: "2026-06-20",
            all_day: true,
            duration: Some("3h"),
            ..fields()
        };
        assert!(build_event_ics("UID1", "20260601T000000Z", &f).is_err());
    }

    #[test]
    fn test_build_event_rrule() {
        let f = EventFields {
            rrule: Some("freq=weekly;byday=sa"),
            ..fields()
        };
        let ics = build_event_ics("UID1", "20260601T000000Z", &f).unwrap();
        assert!(ics.contains("RRULE:FREQ=WEEKLY;BYDAY=SA\r\n"));
        let bad = EventFields {
            rrule: Some("WEEKLY"),
            ..fields()
        };
        assert!(build_event_ics("UID1", "20260601T000000Z", &bad).is_err());
    }

    #[test]
    fn test_build_event_rejects_end_and_duration() {
        let f = EventFields {
            end: Some("2026-06-20T15:00:00"),
            duration: Some("1h"),
            ..fields()
        };
        assert!(build_event_ics("UID1", "20260601T000000Z", &f).is_err());
    }

    // -- calendar discovery --------------------------------------------------

    const SAMPLE_PROPFIND: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<d:multistatus xmlns:d="DAV:" xmlns:cal="urn:ietf:params:xml:ns:caldav">
  <d:response>
    <d:href>/dav/calendars/user/me@example.com/</d:href>
    <d:propstat>
      <d:prop>
        <d:displayname>me@example.com</d:displayname>
        <d:resourcetype><d:collection/></d:resourcetype>
      </d:prop>
      <d:status>HTTP/1.1 200 OK</d:status>
    </d:propstat>
  </d:response>
  <d:response>
    <d:href>/dav/calendars/user/me@example.com/abc123/</d:href>
    <d:propstat>
      <d:prop>
        <d:displayname>Family</d:displayname>
        <d:resourcetype><d:collection/><cal:calendar/></d:resourcetype>
      </d:prop>
      <d:status>HTTP/1.1 200 OK</d:status>
    </d:propstat>
  </d:response>
  <d:response>
    <d:href>/dav/calendars/user/me@example.com/def456/</d:href>
    <d:propstat>
      <d:prop>
        <d:displayname>Work</d:displayname>
        <d:resourcetype><d:collection/><cal:calendar/></d:resourcetype>
      </d:prop>
      <d:status>HTTP/1.1 200 OK</d:status>
    </d:propstat>
  </d:response>
</d:multistatus>"#;

    #[test]
    fn test_parse_calendars_response() {
        let cals = parse_calendars_response(SAMPLE_PROPFIND, "me@example.com").unwrap();
        assert_eq!(cals.len(), 2);
        assert_eq!(cals[0].name, "Family");
        assert_eq!(cals[0].href, "/dav/calendars/user/me@example.com/abc123/");
        assert_eq!(cals[1].name, "Work");
    }

    #[test]
    fn test_match_calendar_case_insensitive() {
        let cals = vec![
            Calendar {
                href: "/a/".into(),
                name: "Family".into(),
            },
            Calendar {
                href: "/b/".into(),
                name: "Work".into(),
            },
        ];
        assert_eq!(match_calendar(&cals, "family").unwrap().href, "/a/");
        assert_eq!(match_calendar(&cals, "WORK").unwrap().href, "/b/");
        assert!(match_calendar(&cals, "Personal").is_none());
    }
}
