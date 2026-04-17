use tauri::{Emitter, Manager};

use crate::asr;
use crate::error::{ParaError, Result};
use crate::llm::recipe;
use crate::services::{folders, keys, meeting_notes, retrieval};
use crate::state::{AppState, CaptureHandle, SessionContext};
use crate::store::db::{
    FolderRow, MeetingDetailRow, MeetingWithNotes, NoteRow, OrgSharedKeyStatusRow, OrganizationRow,
    ParticipantRow, SegmentRow, StandaloneNoteRow, StructuredMeetingNote,
};

use serde::Serialize;

// ---- start_capture ----

#[derive(Debug, Serialize)]
pub struct StartCaptureResp {
    pub meeting_id: String,
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
    let mut guard = state.capture.lock().unwrap();
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
        let mut capture = managed_state.capture.lock().unwrap();
        if capture.as_ref().map(|handle| handle.meeting_id.as_str()) == Some(mid.as_str()) {
            capture.take();
        }
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
    let mut guard = state.capture.lock().unwrap();
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

// ---- run_recipe ----

#[derive(Debug, Serialize)]
pub struct RunRecipeResp {
    pub text: String,
}

/// IPC: run_recipe { meetingId, recipeId: "summary" }
#[tauri::command]
pub fn run_recipe(
    state: tauri::State<'_, AppState>,
    meeting_id: String,
    recipe_id: String,
) -> Result<RunRecipeResp> {
    let out = recipe::run_recipe(&state, &meeting_id, &recipe_id)?;
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

const ALLOWED_PROVIDERS: &[&str] = &["deepgram", "assemblyai", "openai", "anthropic"];

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
pub fn get_session_context(state: tauri::State<'_, AppState>) -> SessionContextResp {
    let session = state.session.lock().unwrap().clone();
    SessionContextResp { session }
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
    let mut session = state.session.lock().unwrap();
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
) -> Result<FolderRow> {
    folders::create_folder(&state.store, &name, &kind, color.as_deref())
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
        .update_folder(folder_id, &name, &kind, color.as_deref())
}

#[tauri::command]
pub fn delete_folder(state: tauri::State<'_, AppState>, folder_id: i64) -> Result<()> {
    state.store.delete_folder(folder_id)
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
    state.store.update_standalone_note(note_id, &title, &text)
}

#[tauri::command]
pub fn list_standalone_notes(state: tauri::State<'_, AppState>) -> Result<Vec<StandaloneNoteRow>> {
    state.store.list_standalone_notes()
}

#[tauri::command]
pub fn delete_standalone_note(state: tauri::State<'_, AppState>, note_id: i64) -> Result<()> {
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
