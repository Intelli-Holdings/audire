pub mod error;
pub mod state;
mod ipc;

pub mod audio;
pub mod asr;
pub mod store;
pub mod keyvault;
pub mod llm;

use state::AppState;

pub fn run() {
    tauri::Builder::default()
        .manage(AppState::new().expect("failed to init AppState"))
        .invoke_handler(tauri::generate_handler![
            ipc::start_capture,
            ipc::stop_capture,
            ipc::append_note,
            ipc::run_recipe,
            ipc::export,
            ipc::save_api_key,
            ipc::has_api_key,
            ipc::delete_api_key,
            ipc::list_meetings,
            ipc::get_notes,
            ipc::list_all_notes,
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running Para-audio");
}
