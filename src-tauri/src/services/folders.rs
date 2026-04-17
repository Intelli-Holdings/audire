use serde::Serialize;

use crate::error::Result;
use crate::store::db::{FolderRow, LocalStore, MeetingWithNotes, StandaloneNoteRow};

#[derive(Debug, Serialize)]
pub struct FolderDetail {
    pub folder: FolderRow,
    pub meetings: Vec<MeetingWithNotes>,
    pub standalone_notes: Vec<StandaloneNoteRow>,
}

pub fn create_folder(
    store: &LocalStore,
    name: &str,
    kind: &str,
    color: Option<&str>,
) -> Result<FolderRow> {
    store.create_folder(name, kind, color, "local_only", None, None)
}

pub fn get_folder_detail(store: &LocalStore, folder_id: i64) -> Result<FolderDetail> {
    let folder = store
        .list_folders()?
        .into_iter()
        .find(|folder| folder.id == folder_id)
        .ok_or_else(|| {
            crate::error::ParaError::InvalidState(format!("unknown folder_id: {}", folder_id))
        })?;

    Ok(FolderDetail {
        folder,
        meetings: store.list_meetings_for_folder(folder_id)?,
        standalone_notes: store.list_standalone_notes_for_folder(folder_id)?,
    })
}
