use crate::caldav::CalDavClient;
use crate::config::Config;
use crate::models::Output;

/// Build a CalDAV client from config (same app-password auth as CardDAV).
pub(crate) fn make_caldav_client() -> anyhow::Result<CalDavClient> {
    let config = Config::load()?;
    let username = config.get_username()?;
    let app_password = config.get_app_password()?;
    Ok(CalDavClient::new(username, app_password))
}

/// List all calendars
pub async fn list_calendars() -> anyhow::Result<()> {
    let client = make_caldav_client()?;
    let calendars = client.list_calendars().await?;
    Output::success(calendars).print();
    Ok(())
}
