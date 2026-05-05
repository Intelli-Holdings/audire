use tauri::{Emitter, Manager};

use crate::asr;
use crate::error::{ParaError, Result};
use crate::llm::recipe;
use crate::llm::provider::LlmProviderInfo;
use crate::services::{calendar, folders, keys, meeting_notes, retrieval};
use crate::state::{AppState, CaptureHandle, SessionContext};
use crate::store::db::{
    CalendarConfigRow, DetectionCalendarPromptRow, DetectionSettingsRow, EventAttendee,
    FolderRow, MeetingDetailRow, MeetingWithNotes, NoteRow, OrgSharedKeyStatusRow,
    OrganizationRow, ParticipantRow, SegmentRow, StandaloneNoteRow, StructuredMeetingNote,
    UpcomingCalendarEventRow,
};

use serde::Serialize;
use std::sync::Mutex;

/// Lock a mutex gracefully, returning a ParaError instead of panicking on poison.
fn lock_mutex<T>(mutex: &Mutex<T>) -> Result<std::sync::MutexGuard<'_, T>> {
    mutex
        .lock()
        .map_err(|e| ParaError::Other(format!("mutex lock failed: {}", e)))
}

// ---- start_capture ----

#[derive(Debug, Serialize)]
pub struct StartCaptureResp {
    pub meeting_id: String,
}

// ---- Calendar Integrations ----

#[tauri::command]
pub fn list_calendar_statuses(state: tauri::State<'_, AppState>) -> Result<Vec<CalendarConfigRow>> {
    calendar::list_provider_statuses(&state.store, &state.keyvault)
}

#[tauri::command]
pub fn save_calendar_config(
    state: tauri::State<'_, AppState>,
    provider: String,
    client_id: String,
    client_secret: Option<String>,
    tenant_id: Option<String>,
) -> Result<()> {
    calendar::save_provider_config(
        &state.keyvault,
        &provider,
        &client_id,
        client_secret.as_deref(),
        tenant_id.as_deref(),
    )
}

#[tauri::command]
pub fn disconnect_calendar_provider(
    state: tauri::State<'_, AppState>,
    provider: String,
) -> Result<()> {
    calendar::disconnect_provider(&state.store, &state.keyvault, &provider)
}

#[tauri::command]
pub async fn connect_calendar_provider(
    state: tauri::State<'_, AppState>,
    provider: String,
) -> Result<CalendarConfigRow> {
    calendar::connect_provider(&state.store, &state.keyvault, &provider).await?;
    let statuses = calendar::list_provider_statuses(&state.store, &state.keyvault)?;
    statuses
        .into_iter()
        .find(|row| row.provider == provider)
        .ok_or_else(|| ParaError::Other("calendar provider status missing after connect".into()))
}

#[tauri::command]
pub async fn list_upcoming_calendar_events(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<UpcomingCalendarEventRow>> {
    calendar::list_upcoming_events(&state.store, &state.keyvault).await
}

/// IPC: start_capture { provider, mode?, includeMic?, targetProcess? }
/// Starts audio capture and connects to the streaming ASR provider.
/// BYOK: API key fetched from env or OS keyring. Never returned to frontend.
///
/// SECURITY: No secrets are included in the response or emitted events.
#[tauri::command]
pub fn start_capture(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    provider: String,
    mode: Option<String>,
    include_mic: Option<bool>,
    target_process: Option<u32>,
) -> Result<StartCaptureResp> {
    let mut guard = lock_mutex(&state.capture)?;
    if guard.is_some() {
        return Err(ParaError::InvalidState("capture already running".into()));
    }

    let provider = provider.to_lowercase();

    // BYOK: keys come from env vars or OS keyring; never returned to frontend.
    // Mock provider doesn't need a key.
    let asr_key = if provider == "mock" {
        String::new()
    } else {
        state
            .keyvault
            .get_provider_key(&provider)
            .ok_or_else(|| ParaError::MissingKey(provider.clone()))?
    };

    // Create meeting record in encrypted local store only after config is valid.
    let meeting_id = state.store.create_meeting(&provider)?;

    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();

    let config = asr::CaptureConfig {
        provider: provider.clone(),
        include_mic: include_mic.unwrap_or(true),
        mode: mode.unwrap_or_else(|| "system".into()),
        target_process,
    };

    // Spawn the capture + ASR pipeline on the dedicated tuned Tokio runtime.
    let mid = meeting_id.clone();
    let store = state.store.clone();
    let app_handle = app.clone();

    state.rt.spawn(async move {
        let _ = app_handle.emit(
            "asr:status",
            serde_json::json!({ "status": "starting pipeline" }),
        );

        let run_result = asr::run_pipeline(
            app_handle.clone(),
            store.clone(),
            mid.clone(),
            config,
            asr_key,
            stop_rx,
        )
        .await;

        if let Err(e) = run_result {
            let _ = store.end_meeting(&mid);
            let _ = app_handle.emit(
                "asr:status",
                serde_json::json!({ "status": format!("pipeline error: {}", e) }),
            );
            let _ = app_handle.emit(
                "asr:lifecycle",
                serde_json::json!({
                    "state": "error",
                    "meeting_id": mid.clone(),
                    "message": e.to_string(),
                }),
            );
        }

        let managed_state = app_handle.state::<AppState>();
        if let Ok(mut capture) = managed_state.capture.lock() {
            if capture.as_ref().map(|handle| handle.meeting_id.as_str()) == Some(mid.as_str()) {
                capture.take();
            }
        };
    });

    *guard = Some(CaptureHandle {
        meeting_id: meeting_id.clone(),
        stop: stop_tx,
    });

    Ok(StartCaptureResp { meeting_id })
}

// ---- stop_capture ----

/// IPC: stop_capture { meetingId }
/// Sends stop signal to pipeline, which triggers Finalize/Terminate on ASR.
#[tauri::command]
pub fn stop_capture(state: tauri::State<'_, AppState>, meeting_id: String) -> Result<()> {
    let mut guard = lock_mutex(&state.capture)?;
    let handle = guard
        .take()
        .ok_or_else(|| ParaError::InvalidState("no capture running".into()))?;

    if handle.meeting_id != meeting_id {
        return Err(ParaError::InvalidState("meeting_id mismatch".into()));
    }

    let _ = handle.stop.send(());
    state.store.end_meeting(&meeting_id)?;

    Ok(())
}

// ---- append_note ----

/// IPC: append_note { meetingId, text }
/// Notes are the user's "lead" — Audire combines them with transcript during recipes.
#[tauri::command]
pub fn append_note(
    state: tauri::State<'_, AppState>,
    meeting_id: String,
    text: String,
) -> Result<()> {
    state.store.insert_note(&meeting_id, &text)?;
    Ok(())
}

#[tauri::command]
pub fn replace_meeting_notes(
    state: tauri::State<'_, AppState>,
    meeting_id: String,
    text: String,
) -> Result<()> {
    state.store.replace_meeting_notes(&meeting_id, &text)
}

#[tauri::command]
pub fn delete_meeting(
    state: tauri::State<'_, AppState>,
    meeting_id: String,
) -> Result<()> {
    state.store.delete_meeting(&meeting_id)
}

// ---- run_recipe ----

#[derive(Debug, Serialize)]
pub struct RunRecipeResp {
    pub text: String,
}

/// IPC: run_recipe { meetingId, recipeId }
#[tauri::command]
pub async fn run_recipe(
    state: tauri::State<'_, AppState>,
    meeting_id: String,
    recipe_id: String,
) -> Result<RunRecipeResp> {
    let out = recipe::run_recipe(&state, &meeting_id, &recipe_id).await?;
    Ok(RunRecipeResp { text: out })
}

// ---- export ----

#[derive(Debug, Serialize)]
pub struct ExportResp {
    pub path: String,
}

/// IPC: export { meetingId, format: "md" }
/// Returns the file path within the app data directory (no external paths).
#[tauri::command]
pub fn export(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    meeting_id: String,
    format: String,
) -> Result<ExportResp> {
    let _ = format; // MVP only supports markdown

    let app_data = match app.path().app_data_dir() {
        Ok(path) => path,
        Err(e) => return Err(ParaError::Other(e.to_string())),
    };

    let out = state.store.export_meeting_markdown(&meeting_id, app_data)?;

    Ok(ExportResp {
        path: out.display().to_string(),
    })
}

#[derive(Debug, Serialize)]
pub struct MeetingDetailResp {
    pub meeting: MeetingDetailRow,
    pub user_notes: Vec<NoteRow>,
    pub segments: Vec<SegmentRow>,
    pub structured_note: Option<StructuredMeetingNote>,
}

#[derive(Debug, Serialize)]
pub struct SessionContextResp {
    pub session: SessionContext,
}

// ---- API key management ----

const ALLOWED_PROVIDERS: &[&str] = &["deepgram", "assemblyai", "openai", "anthropic", "gemini"];

/// IPC: save_api_key { provider, key }
/// Stores a provider API key in the OS keyring.
/// SECURITY: validates provider against allowlist.
#[tauri::command]
pub fn save_api_key(
    state: tauri::State<'_, AppState>,
    provider: String,
    key: String,
) -> Result<()> {
    let provider = provider.to_lowercase();
    if !ALLOWED_PROVIDERS.contains(&provider.as_str()) {
        return Err(ParaError::KeyVault(format!(
            "unknown provider: {}",
            provider
        )));
    }
    state
        .keyvault
        .set_provider_key(&provider, &key)
        .map_err(|e| ParaError::KeyVault(e.to_string()))?;
    Ok(())
}

/// IPC: has_api_key { provider } -> bool
/// Returns whether a key exists. Never returns the key itself.
#[tauri::command]
pub fn has_api_key(state: tauri::State<'_, AppState>, provider: String) -> Result<bool> {
    Ok(state.keyvault.has_provider_key(&provider.to_lowercase()))
}

/// IPC: delete_api_key { provider }
#[tauri::command]
pub fn delete_api_key(state: tauri::State<'_, AppState>, provider: String) -> Result<()> {
    let provider = provider.to_lowercase();
    if !ALLOWED_PROVIDERS.contains(&provider.as_str()) {
        return Err(ParaError::KeyVault(format!(
            "unknown provider: {}",
            provider
        )));
    }
    state
        .keyvault
        .delete_provider_key(&provider)
        .map_err(|e| ParaError::KeyVault(e.to_string()))?;
    Ok(())
}

#[tauri::command]
pub fn get_session_context(state: tauri::State<'_, AppState>) -> Result<SessionContextResp> {
    let session = lock_mutex(&state.session)?.clone();
    Ok(SessionContextResp { session })
}

#[tauri::command]
pub fn set_session_context(
    state: tauri::State<'_, AppState>,
    mode: String,
    user_id: Option<String>,
    email: Option<String>,
    active_org_id: Option<String>,
) -> Result<()> {
    state.store.set_session_context(
        &mode,
        user_id.as_deref(),
        email.as_deref(),
        active_org_id.as_deref(),
    )?;
    let mut session = lock_mutex(&state.session)?;
    *session = SessionContext {
        mode,
        user_id,
        email,
        active_org_id,
    };
    Ok(())
}

#[tauri::command]
pub fn resolve_provider_key_source(
    state: tauri::State<'_, AppState>,
    provider: String,
    org_id: Option<i64>,
) -> keys::KeyResolutionStatus {
    keys::resolve_provider_source(&state.keyvault, &provider.to_lowercase(), org_id)
}

#[tauri::command]
pub fn list_org_shared_key_statuses(
    state: tauri::State<'_, AppState>,
    org_id: i64,
) -> Result<Vec<OrgSharedKeyStatusRow>> {
    keys::list_org_key_statuses(&state.store, org_id)
}

#[tauri::command]
pub fn save_org_api_key(
    state: tauri::State<'_, AppState>,
    org_id: i64,
    provider: String,
    key: String,
) -> Result<()> {
    let provider = provider.to_lowercase();
    if !ALLOWED_PROVIDERS.contains(&provider.as_str()) {
        return Err(ParaError::KeyVault(format!(
            "unknown provider: {}",
            provider
        )));
    }
    keys::save_org_key(&state.store, &state.keyvault, org_id, &provider, &key)
}

#[tauri::command]
pub fn delete_org_api_key(
    state: tauri::State<'_, AppState>,
    org_id: i64,
    provider: String,
) -> Result<()> {
    let provider = provider.to_lowercase();
    if !ALLOWED_PROVIDERS.contains(&provider.as_str()) {
        return Err(ParaError::KeyVault(format!(
            "unknown provider: {}",
            provider
        )));
    }
    keys::delete_org_key(&state.store, &state.keyvault, org_id, &provider)
}

// ---- Meetings & Notes ----

#[tauri::command]
pub fn list_meeting_templates() -> Vec<meeting_notes::MeetingTemplateOption> {
    meeting_notes::template_options()
}

#[tauri::command]
pub fn list_meetings(state: tauri::State<'_, AppState>) -> Result<Vec<MeetingWithNotes>> {
    state.store.list_meetings()
}

#[tauri::command]
pub fn get_meeting_detail(
    state: tauri::State<'_, AppState>,
    meeting_id: String,
) -> Result<MeetingDetailResp> {
    Ok(MeetingDetailResp {
        meeting: state.store.get_meeting(&meeting_id)?,
        user_notes: state.store.get_notes_for_meeting(&meeting_id)?,
        segments: state.store.segments_for_meeting(&meeting_id)?,
        structured_note: state.store.get_structured_meeting_note(&meeting_id)?,
    })
}

#[tauri::command]
pub fn list_segments(
    state: tauri::State<'_, AppState>,
    meeting_id: String,
) -> Result<Vec<SegmentRow>> {
    state.store.segments_for_meeting(&meeting_id)
}

#[tauri::command]
pub fn get_notes(state: tauri::State<'_, AppState>, meeting_id: String) -> Result<Vec<NoteRow>> {
    state.store.get_notes_for_meeting(&meeting_id)
}

#[tauri::command]
pub fn list_all_notes(state: tauri::State<'_, AppState>) -> Result<Vec<NoteRow>> {
    state.store.list_all_notes()
}

#[tauri::command]
pub fn set_meeting_template(
    state: tauri::State<'_, AppState>,
    meeting_id: String,
    template_kind: String,
) -> Result<()> {
    let normalized = meeting_notes::normalize_template_kind(Some(&template_kind));
    state.store.set_meeting_template(&meeting_id, &normalized)
}

#[tauri::command]
pub fn update_meeting_title(
    state: tauri::State<'_, AppState>,
    meeting_id: String,
    title: String,
) -> Result<()> {
    state.store.update_meeting_title(&meeting_id, &title)
}

#[tauri::command]
pub fn generate_structured_meeting_notes(
    state: tauri::State<'_, AppState>,
    meeting_id: String,
    template_kind: Option<String>,
) -> Result<StructuredMeetingNote> {
    meeting_notes::generate_and_store(&state.store, &meeting_id, template_kind.as_deref())
}

#[tauri::command]
pub fn get_structured_meeting_notes(
    state: tauri::State<'_, AppState>,
    meeting_id: String,
) -> Result<Option<StructuredMeetingNote>> {
    state.store.get_structured_meeting_note(&meeting_id)
}

#[tauri::command]
pub fn update_structured_note_summary(
    state: tauri::State<'_, AppState>,
    meeting_id: String,
    summary: String,
) -> Result<()> {
    state
        .store
        .update_structured_note_summary(&meeting_id, &summary)
}

#[tauri::command]
pub fn update_structured_note_item(
    state: tauri::State<'_, AppState>,
    item_id: i64,
    text: String,
) -> Result<()> {
    state.store.update_structured_note_item(item_id, &text)
}

#[tauri::command]
pub fn ask_audire(
    state: tauri::State<'_, AppState>,
    query: String,
    scope: Option<String>,
    meeting_id: Option<String>,
    folder_id: Option<i64>,
) -> Result<retrieval::AskAudireResp> {
    retrieval::ask(
        &state.store,
        &query,
        scope.as_deref().unwrap_or("all"),
        meeting_id.as_deref(),
        folder_id,
    )
}

#[tauri::command]
pub async fn ask_audire_llm(
    state: tauri::State<'_, AppState>,
    query: String,
    scope: Option<String>,
    meeting_id: Option<String>,
    folder_id: Option<i64>,
) -> Result<retrieval::AskAudireResp> {
    retrieval::ask_with_llm(
        &state.store,
        &state.keyvault,
        &query,
        scope.as_deref().unwrap_or("all"),
        meeting_id.as_deref(),
        folder_id,
    )
    .await
}

// ---- Folders ----

#[tauri::command]
pub fn list_folders(state: tauri::State<'_, AppState>) -> Result<Vec<FolderRow>> {
    state.store.list_folders()
}

#[tauri::command]
pub fn create_folder(
    state: tauri::State<'_, AppState>,
    name: String,
    kind: String,
    color: Option<String>,
    description: Option<String>,
) -> Result<FolderRow> {
    folders::create_folder(&state.store, &name, &kind, color.as_deref(), description.as_deref())
}

#[tauri::command]
pub fn update_folder(
    state: tauri::State<'_, AppState>,
    folder_id: i64,
    name: String,
    kind: String,
    color: Option<String>,
) -> Result<()> {
    state
        .store
        .update_folder(folder_id, &name, &kind, color.as_deref())?;
    if state.sync_runtime.is_unlocked() {
        let _ = state.sync_runtime.with_kek(|kek| {
            crate::sync::api::enqueue_folder_upsert(&state.store, folder_id, kek)
        });
    }
    Ok(())
}

#[tauri::command]
pub fn delete_folder(state: tauri::State<'_, AppState>, folder_id: i64) -> Result<()> {
    if state.sync_runtime.is_unlocked() {
        let _ = state.sync_runtime.with_kek(|kek| {
            crate::sync::api::enqueue_folder_delete(&state.store, folder_id, kek)
        });
    }
    state.store.delete_folder(folder_id)
}

/// `share_folder_with_org` — bind a folder to an org's default vault
/// and enqueue the initial folder + child note ops so other org
/// members see the shared content.
#[tauri::command]
pub fn share_folder_with_org(
    state: tauri::State<'_, AppState>,
    folder_id: i64,
    org_id: String,
) -> Result<String> {
    let kek = state.sync_runtime.with_kek(|kek| Ok(kek.clone()))?;
    crate::sync::api::share_folder_with_org(&state.store, folder_id, &org_id, &kek)
}

#[tauri::command]
pub fn get_folder_detail(
    state: tauri::State<'_, AppState>,
    folder_id: i64,
) -> Result<folders::FolderDetail> {
    folders::get_folder_detail(&state.store, folder_id)
}

#[tauri::command]
pub fn assign_meeting_folder(
    state: tauri::State<'_, AppState>,
    meeting_id: String,
    folder_id: Option<i64>,
) -> Result<()> {
    state.store.assign_meeting_folder(&meeting_id, folder_id)
}

#[tauri::command]
pub fn assign_standalone_note_folder(
    state: tauri::State<'_, AppState>,
    note_id: i64,
    folder_id: Option<i64>,
) -> Result<()> {
    state
        .store
        .assign_standalone_note_folder(note_id, folder_id)
}

// ---- Standalone Notes ----

#[tauri::command]
pub fn create_standalone_note(
    state: tauri::State<'_, AppState>,
    title: String,
) -> Result<StandaloneNoteRow> {
    state.store.create_standalone_note(&title)
}

#[tauri::command]
pub fn get_standalone_note(
    state: tauri::State<'_, AppState>,
    note_id: i64,
) -> Result<StandaloneNoteRow> {
    state.store.get_standalone_note(note_id)
}

#[tauri::command]
pub fn update_standalone_note(
    state: tauri::State<'_, AppState>,
    note_id: i64,
    title: String,
    text: String,
) -> Result<()> {
    state.store.update_standalone_note(note_id, &title, &text)?;
    if state.sync_runtime.is_unlocked() {
        let _ = state.sync_runtime.with_kek(|kek| {
            crate::sync::api::enqueue_note_upsert(&state.store, note_id, &title, &text, kek)
        });
    }
    Ok(())
}

#[tauri::command]
pub fn list_standalone_notes(state: tauri::State<'_, AppState>) -> Result<Vec<StandaloneNoteRow>> {
    state.store.list_standalone_notes()
}

#[tauri::command]
pub fn delete_standalone_note(state: tauri::State<'_, AppState>, note_id: i64) -> Result<()> {
    if state.sync_runtime.is_unlocked() {
        let _ = state.sync_runtime.with_kek(|kek| {
            crate::sync::api::enqueue_note_delete(&state.store, note_id, kek)
        });
    }
    state.store.delete_standalone_note(note_id)
}

// ---- Participants ----

#[tauri::command]
pub fn add_participant(
    state: tauri::State<'_, AppState>,
    meeting_id: Option<String>,
    name: String,
    email: Option<String>,
) -> Result<ParticipantRow> {
    let p = state
        .store
        .add_participant(&name, email.as_deref(), "manual")?;
    if let Some(mid) = meeting_id {
        state.store.link_participant_meeting(&mid, p.id)?;
    }
    Ok(p)
}

#[tauri::command]
pub fn list_participants(
    state: tauri::State<'_, AppState>,
    meeting_id: Option<String>,
) -> Result<Vec<ParticipantRow>> {
    match meeting_id {
        Some(mid) => state.store.list_participants_for_meeting(&mid),
        None => state.store.list_all_participants(),
    }
}

#[tauri::command]
pub fn list_all_participants(state: tauri::State<'_, AppState>) -> Result<Vec<ParticipantRow>> {
    state.store.list_all_participants()
}

// ---- Organizations ----

#[tauri::command]
pub fn add_organization(
    state: tauri::State<'_, AppState>,
    name: String,
    domain: Option<String>,
) -> Result<OrganizationRow> {
    state.store.add_organization(&name, domain.as_deref())
}

#[tauri::command]
pub fn list_organizations(state: tauri::State<'_, AppState>) -> Result<Vec<OrganizationRow>> {
    state.store.list_organizations()
}

// ---- Auto-populate People & Companies from Calendar Attendees ----

#[tauri::command]
pub fn import_event_attendees(
    state: tauri::State<'_, AppState>,
    meeting_id: String,
    attendees: Vec<EventAttendee>,
) -> Result<()> {
    state.store.import_event_attendees(&meeting_id, &attendees)
}

// ---- LLM Provider Management ----

#[tauri::command]
pub fn list_llm_providers(state: tauri::State<'_, AppState>) -> Vec<LlmProviderInfo> {
    state.llm_registry.list(&state.keyvault)
}

#[tauri::command]
pub fn set_preferred_llm_provider(
    state: tauri::State<'_, AppState>,
    provider_id: String,
) -> Result<()> {
    let mut settings = state.store.get_detection_settings()?;
    settings.preferred_llm_provider = provider_id;
    state.store.update_detection_settings(&settings)
}

#[tauri::command]
pub fn save_ollama_endpoint(
    state: tauri::State<'_, AppState>,
    endpoint: String,
    model: Option<String>,
) -> Result<()> {
    let mut settings = state.store.get_detection_settings()?;
    settings.ollama_endpoint = endpoint;
    if let Some(m) = model {
        settings.ollama_model = m;
    }
    state.store.update_detection_settings(&settings)
}

#[tauri::command]
pub async fn test_llm_provider(
    state: tauri::State<'_, AppState>,
    provider_id: String,
) -> Result<String> {
    state
        .llm_registry
        .test_provider(&state.keyvault, &provider_id)
        .await
        .map_err(|e| ParaError::Other(e))
}

// ---- Detection Settings ----

#[tauri::command]
pub fn get_detection_settings(state: tauri::State<'_, AppState>) -> Result<DetectionSettingsRow> {
    state.store.get_detection_settings()
}

#[tauri::command]
pub fn update_detection_settings(
    state: tauri::State<'_, AppState>,
    settings: DetectionSettingsRow,
) -> Result<()> {
    state.store.update_detection_settings(&settings)
}

#[tauri::command]
pub fn list_detection_prompts(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<DetectionCalendarPromptRow>> {
    state.store.list_detection_prompts()
}

#[tauri::command]
pub fn respond_to_detection_prompt(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    external_id: String,
    provider: String,
    action: String,
    attendees: Option<Vec<EventAttendee>>,
) -> Result<Option<String>> {
    match action.as_str() {
        "accept" => {
            // Start capture for this meeting
            let asr_provider = "assemblyai".to_string(); // default ASR
            let asr_key = state
                .keyvault
                .get_provider_key(&asr_provider)
                .or_else(|| state.keyvault.get_provider_key("deepgram"))
                .ok_or_else(|| ParaError::MissingKey("No ASR key configured".into()))?;

            let actual_provider = if state.keyvault.has_provider_key("assemblyai") {
                "assemblyai"
            } else {
                "deepgram"
            };

            let meeting_id =
                state
                    .store
                    .create_meeting_with_calendar(actual_provider, &external_id, &provider)?;

            // Auto-populate people & companies from calendar attendees
            if let Some(ref att) = attendees {
                let _ = state.store.import_event_attendees(&meeting_id, att);
            }

            // Mark prompt as accepted
            state.store.update_detection_prompt_action(
                &external_id,
                &provider,
                "accepted",
                Some(&meeting_id),
            )?;

            // Start capture pipeline
            let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
            let config = crate::asr::CaptureConfig {
                provider: actual_provider.to_string(),
                include_mic: true,
                mode: "system".into(),
                target_process: None,
            };

            let mid = meeting_id.clone();
            let store = state.store.clone();
            let app_handle = app.clone();

            state.rt.spawn(async move {
                let _ = app_handle.emit(
                    "asr:status",
                    serde_json::json!({ "status": "starting pipeline" }),
                );

                let run_result = crate::asr::run_pipeline(
                    app_handle.clone(),
                    store.clone(),
                    mid.clone(),
                    config,
                    asr_key,
                    stop_rx,
                )
                .await;

                if let Err(e) = run_result {
                    let _ = store.end_meeting(&mid);
                    let _ = app_handle.emit(
                        "asr:status",
                        serde_json::json!({ "status": format!("pipeline error: {}", e) }),
                    );
                    let _ = app_handle.emit(
                        "asr:lifecycle",
                        serde_json::json!({
                            "state": "error",
                            "meeting_id": mid.clone(),
                            "message": e.to_string(),
                        }),
                    );
                }

                let managed_state = app_handle.state::<AppState>();
                if let Ok(mut capture) = managed_state.capture.lock() {
                    if capture.as_ref().map(|h| h.meeting_id.as_str()) == Some(mid.as_str()) {
                        capture.take();
                    }
                };
            });

            let mut guard = lock_mutex(&state.capture)?;
            *guard = Some(CaptureHandle {
                meeting_id: meeting_id.clone(),
                stop: stop_tx,
            });

            Ok(Some(meeting_id))
        }
        "dismiss" | "expired" => {
            state.store.update_detection_prompt_action(
                &external_id,
                &provider,
                &action,
                None,
            )?;
            Ok(None)
        }
        _ => Err(ParaError::Other(format!("Invalid action: {}", action))),
    }
}

// ---- Detector Lifecycle ----

#[tauri::command]
pub fn start_detector(app: tauri::AppHandle, state: tauri::State<'_, AppState>) -> Result<()> {
    let mut guard = lock_mutex(&state.detector)?;
    if guard.is_some() {
        return Ok(()); // Already running
    }

    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let store = state.store.clone();
    let keyvault = state.keyvault.clone();
    let app_handle = app.clone();

    state.rt.spawn(async move {
        crate::services::detection::run_detection_loop(app_handle, store, keyvault, stop_rx).await;
    });

    *guard = Some(crate::state::DetectorHandle { stop: stop_tx });
    Ok(())
}

#[tauri::command]
pub fn stop_detector(state: tauri::State<'_, AppState>) -> Result<()> {
    let mut guard = lock_mutex(&state.detector)?;
    if let Some(handle) = guard.take() {
        let _ = handle.stop.send(());
    }
    Ok(())
}

// ---- Cloud sync (optional) ----

#[tauri::command]
pub fn sync_account_status(
    state: tauri::State<'_, AppState>,
) -> Result<crate::sync::AccountStatus> {
    crate::sync::account::account_status(&state.store)
}

#[tauri::command]
pub async fn sync_sign_up(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    request: crate::sync::SignUpRequest,
) -> Result<crate::sync::account::RecoveryReveal> {
    let reveal = crate::sync::account::sign_up(&state.store, &request).await?;
    // After sign-up the KEK is the one we just derived from the
    // passphrase. Stash it so the worker manager can immediately start.
    let salt: Vec<u8> = state
        .store
        .with_conn(|c| {
            c.query_row(
                "SELECT kek_salt FROM sync_account WHERE id = 1",
                [],
                |r| r.get::<_, Vec<u8>>(0),
            )
        })
        .map_err(|e| ParaError::Db(format!("read kek_salt: {e}")))?;
    let kek = crate::sync::crypto::derive_kek(&request.passphrase, &salt)
        .map_err(|e| ParaError::Other(e.to_string()))?;
    state.sync_runtime.unlock(kek);
    state
        .sync_runtime
        .start_workers(&state.rt, &app, &state.store, &request.server_url, &request.access_token)?;
    Ok(reveal)
}

#[tauri::command]
pub async fn sync_sign_in(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    request: crate::sync::SignInRequest,
) -> Result<()> {
    crate::sync::account::sign_in(&state.store, &request).await?;
    sync_unlock_inner(&app, &state, &request.passphrase)?;
    Ok(())
}

#[tauri::command]
pub fn sync_sign_out(state: tauri::State<'_, AppState>) -> Result<()> {
    state.sync_runtime.shutdown();
    crate::sync::account::sign_out(&state.store)
}

#[tauri::command]
pub fn sync_unlock(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    passphrase: String,
) -> Result<()> {
    sync_unlock_inner(&app, &state, &passphrase)
}

fn sync_unlock_inner(
    app: &tauri::AppHandle,
    state: &tauri::State<'_, AppState>,
    passphrase: &str,
) -> Result<()> {
    let row = state
        .store
        .with_conn(|c| {
            c.query_row(
                "SELECT kek_salt, server_url, access_token FROM sync_account WHERE id = 1",
                [],
                |r| Ok((
                    r.get::<_, Vec<u8>>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<String>>(2)?,
                )),
            )
        })
        .map_err(|e| ParaError::Db(format!("read sync_account: {e}")))?;
    let (salt, server_url, access_token_opt) = row;
    let access_token = access_token_opt
        .ok_or_else(|| ParaError::InvalidState("no access token saved".into()))?;
    let kek = crate::sync::crypto::derive_kek(passphrase, &salt)
        .map_err(|e| ParaError::Other(e.to_string()))?;
    // Verify the passphrase works by attempting to unwrap the
    // identity key.
    crate::sync::account::unwrap_identity_secret(&state.store, &kek)?;
    state.sync_runtime.unlock(kek);
    state
        .sync_runtime
        .start_workers(&state.rt, app, &state.store, &server_url, &access_token)?;
    Ok(())
}

#[tauri::command]
pub async fn sync_refresh(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<crate::sync::vaults::LocalVaultRow>> {
    let (server_url, access_token) = read_account_endpoint(&state)?;
    let vaults = crate::sync::api::refresh_vaults(&state.store, &server_url, &access_token).await?;
    if state.sync_runtime.is_unlocked() {
        state
            .sync_runtime
            .start_workers(&state.rt, &app, &state.store, &server_url, &access_token)?;
    }
    Ok(vaults)
}

#[tauri::command]
pub async fn sync_create_org(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    args: crate::sync::api::CreateOrgArgs,
) -> Result<crate::sync::api::CreateOrgOutcome> {
    let (server_url, access_token) = read_account_endpoint(&state)?;
    // Need the KEK to wrap the new vault key for self.
    let kek = state.sync_runtime.with_kek(|kek| Ok(kek.clone()))?;
    let outcome =
        crate::sync::api::create_org(&state.store, &server_url, &access_token, &kek, &args).await?;
    state
        .sync_runtime
        .start_workers(&state.rt, &app, &state.store, &server_url, &access_token)?;
    Ok(outcome)
}

#[tauri::command]
pub async fn sync_invite_to_org(
    state: tauri::State<'_, AppState>,
    args: crate::sync::api::InviteToOrgArgs,
) -> Result<crate::sync::api::InviteToOrgOutcome> {
    let (server_url, access_token) = read_account_endpoint(&state)?;
    let kek = state.sync_runtime.with_kek(|kek| Ok(kek.clone()))?;
    crate::sync::api::invite_to_org(&state.store, &server_url, &access_token, &kek, &args).await
}

#[tauri::command]
pub fn sync_list_orgs(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<crate::sync::orgs::LocalOrgRow>> {
    crate::sync::orgs::list(&state.store)
}

#[tauri::command]
pub fn sync_list_vaults(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<crate::sync::vaults::LocalVaultRow>> {
    crate::sync::vaults::list(&state.store)
}

#[tauri::command]
pub fn sync_running_vaults(state: tauri::State<'_, AppState>) -> Result<Vec<String>> {
    Ok(state.sync_runtime.running_vaults())
}

fn read_account_endpoint(state: &tauri::State<'_, AppState>) -> Result<(String, String)> {
    let row: (String, Option<String>) = state
        .store
        .with_conn(|c| {
            c.query_row(
                "SELECT server_url, access_token FROM sync_account WHERE id = 1",
                [],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?)),
            )
        })
        .map_err(|e| ParaError::Db(format!("read sync_account: {e}")))?;
    let access = row
        .1
        .ok_or_else(|| ParaError::InvalidState("no access token saved".into()))?;
    Ok((row.0, access))
}
