// Background detection loop.
//
// Polls calendar events every 30s and emits a prompt to the frontend
// when a meeting is about to start (within `calendar_lead_minutes`).

use tauri::{AppHandle, Emitter};
use tokio::sync::oneshot;

use crate::keyvault::vault::KeyVault;
use crate::services::calendar;
use crate::store::db::LocalStore;

/// Run the detection loop until `stop_rx` fires or the app shuts down.
pub async fn run_detection_loop(
    app_handle: AppHandle,
    store: LocalStore,
    keyvault: KeyVault,
    stop_rx: oneshot::Receiver<()>,
) {
    tokio::pin!(stop_rx);

    loop {
        // Check settings each iteration (user may toggle on/off)
        let settings = match store.get_detection_settings() {
            Ok(s) => s,
            Err(e) => {
                eprintln!("[detection] failed to read settings: {}", e);
                // Wait before retrying
                tokio::select! {
                    _ = tokio::time::sleep(std::time::Duration::from_secs(30)) => continue,
                    _ = &mut stop_rx => break,
                }
            }
        };

        if settings.calendar_detection_enabled {
            if let Err(e) = poll_calendar(&app_handle, &store, &keyvault, &settings).await {
                eprintln!("[detection] calendar poll error: {}", e);
            }
        }

        // Sleep 30s or stop
        tokio::select! {
            _ = tokio::time::sleep(std::time::Duration::from_secs(30)) => {},
            _ = &mut stop_rx => break,
        }
    }

    eprintln!("[detection] loop stopped");
}

async fn poll_calendar(
    app_handle: &AppHandle,
    store: &LocalStore,
    keyvault: &KeyVault,
    settings: &crate::store::db::DetectionSettingsRow,
) -> Result<(), String> {
    let events = calendar::list_upcoming_events(store, keyvault)
        .await
        .map_err(|e| e.to_string())?;

    let now = chrono::Utc::now();
    let lead = chrono::Duration::minutes(settings.calendar_lead_minutes);

    for event in &events {
        // Parse event start time
        let start_str = if event.start.len() == 10 {
            // Bare date (all-day event) — treat as midnight local
            format!("{}T00:00:00", event.start)
        } else {
            event.start.clone()
        };

        let event_start = match chrono::DateTime::parse_from_rfc3339(&start_str) {
            Ok(dt) => dt.with_timezone(&chrono::Utc),
            Err(_) => {
                // Try naive datetime (no timezone) as UTC
                match chrono::NaiveDateTime::parse_from_str(&start_str, "%Y-%m-%dT%H:%M:%S") {
                    Ok(naive) => naive.and_utc(),
                    Err(_) => continue,
                }
            }
        };

        // Check if event is within lead time window
        let diff = event_start - now;
        if diff < chrono::Duration::zero() || diff > lead {
            continue;
        }

        // Check if we already prompted for this event
        match store.get_detection_prompt(&event.external_id, &event.provider) {
            Ok(Some(_)) => continue, // Already prompted
            Ok(None) => {}           // New — proceed
            Err(e) => {
                eprintln!("[detection] failed to check prompt: {}", e);
                continue;
            }
        }

        // Record prompt and emit to frontend
        if let Err(e) = store.upsert_detection_prompt(
            &event.external_id,
            &event.provider,
            &event.title,
            &event.start,
            &event.end,
        ) {
            eprintln!("[detection] failed to upsert prompt: {}", e);
            continue;
        }

        let payload = serde_json::json!({
            "external_id": event.external_id,
            "provider": event.provider,
            "title": event.title,
            "start": event.start,
            "end": event.end,
            "attendees": event.attendees,
        });

        if let Err(e) = app_handle.emit("detection://prompt", &payload) {
            eprintln!("[detection] failed to emit prompt event: {}", e);
        }
    }

    Ok(())
}
