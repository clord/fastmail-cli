use crate::caldav::EventFields;
use crate::commands::calendar::make_caldav_client;
use crate::models::Output;

/// Create a calendar event. `calendar` is resolved by display name
/// (case-insensitive) via calendar discovery.
#[allow(clippy::too_many_arguments)]
pub async fn create_event(
    calendar: &str,
    title: &str,
    start: &str,
    end: Option<&str>,
    duration: Option<&str>,
    all_day: bool,
    location: Option<&str>,
    notes: Option<&str>,
    rrule: Option<&str>,
) -> anyhow::Result<()> {
    let client = make_caldav_client()?;
    let cal = client.find_calendar(calendar).await?;

    let created = client
        .create_event(
            &cal,
            &EventFields {
                title,
                start,
                end,
                duration,
                all_day,
                location,
                notes,
                rrule,
            },
        )
        .await?;

    Output::success(created).print();
    Ok(())
}
