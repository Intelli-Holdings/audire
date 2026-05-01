pub mod error;
mod ipc;
pub mod state;

pub mod asr;
pub mod audio;
pub mod keyvault;
pub mod llm;
pub mod services;
pub mod store;
pub mod sync;

use state::AppState;

pub fn run() {
    // Rustls 0.23+ requires an explicit crypto provider.
    // tokio-tungstenite's "rustls-tls-webpki-roots" feature uses ring.
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("failed to install rustls crypto provider");
    tauri::Builder::default()
        .manage(AppState::new().expect("failed to init AppState"))
        .invoke_handler(tauri::generate_handler![
            ipc::start_capture,
            ipc::stop_capture,
            ipc::append_note,
            ipc::run_recipe,
            ipc::export,
            ipc::list_calendar_statuses,
            ipc::save_calendar_config,
            ipc::disconnect_calendar_provider,
            ipc::connect_calendar_provider,
            ipc::list_upcoming_calendar_events,
            ipc::save_api_key,
            ipc::has_api_key,
            ipc::delete_api_key,
            ipc::get_session_context,
            ipc::set_session_context,
            ipc::resolve_provider_key_source,
            ipc::list_org_shared_key_statuses,
            ipc::save_org_api_key,
            ipc::delete_org_api_key,
            ipc::list_meeting_templates,
            ipc::list_meetings,
            ipc::get_meeting_detail,
            ipc::list_segments,
            ipc::get_notes,
            ipc::list_all_notes,
            ipc::generate_structured_meeting_notes,
            ipc::get_structured_meeting_notes,
            ipc::update_meeting_title,
            ipc::update_structured_note_summary,
            ipc::update_structured_note_item,
            ipc::set_meeting_template,
            ipc::ask_audire,
            ipc::ask_audire_llm,
            ipc::list_folders,
            ipc::create_folder,
            ipc::update_folder,
            ipc::delete_folder,
            ipc::get_folder_detail,
            ipc::assign_meeting_folder,
            ipc::assign_standalone_note_folder,
            ipc::create_standalone_note,
            ipc::get_standalone_note,
            ipc::update_standalone_note,
            ipc::list_standalone_notes,
            ipc::delete_standalone_note,
            ipc::add_participant,
            ipc::list_participants,
            ipc::list_all_participants,
            ipc::add_organization,
            ipc::list_organizations,
            ipc::import_event_attendees,
            ipc::list_llm_providers,
            ipc::set_preferred_llm_provider,
            ipc::save_ollama_endpoint,
            ipc::test_llm_provider,
            ipc::get_detection_settings,
            ipc::update_detection_settings,
            ipc::list_detection_prompts,
            ipc::respond_to_detection_prompt,
            ipc::start_detector,
            ipc::stop_detector,
            ipc::sync_account_status,
            ipc::sync_sign_up,
            ipc::sync_sign_in,
            ipc::sync_sign_out,
            ipc::sync_unlock,
            ipc::sync_refresh,
            ipc::sync_create_org,
            ipc::sync_invite_to_org,
            ipc::sync_list_orgs,
            ipc::sync_list_vaults,
            ipc::sync_running_vaults,
            ipc::share_folder_with_org,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Audire");
}
