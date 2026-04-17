use serde::Serialize;

use crate::error::Result;
use crate::store::db::{
    LocalStore, SegmentSearchHit, StandaloneNoteSearchHit, StructuredItemSearchHit,
};

#[derive(Debug, Clone, Serialize)]
pub struct AskAudireResp {
    pub answer: String,
    pub citations: Vec<AskCitation>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AskCitation {
    pub kind: String,
    pub meeting_id: Option<String>,
    pub note_id: Option<i64>,
    pub item_id: Option<i64>,
    pub segment_id: Option<i64>,
    pub title: String,
    pub excerpt: String,
    pub ts_ms: Option<i64>,
    pub folder_name: Option<String>,
}

pub fn ask(
    store: &LocalStore,
    query: &str,
    scope: &str,
    meeting_id: Option<&str>,
    folder_id: Option<i64>,
) -> Result<AskAudireResp> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return Ok(AskAudireResp {
            answer: "Ask Audire needs a non-empty question.".to_string(),
            citations: Vec::new(),
        });
    }

    let mut citations = Vec::new();

    match scope {
        "meeting" => {
            if let Some(meeting_id) = meeting_id {
                for hit in
                    store.search_segments_for_query(meeting_id, &keyword_query(trimmed), 6)?
                {
                    citations.push(AskCitation {
                        kind: "segment".to_string(),
                        meeting_id: Some(meeting_id.to_string()),
                        note_id: None,
                        item_id: None,
                        segment_id: Some(hit.id),
                        title: "Transcript".to_string(),
                        excerpt: hit.text,
                        ts_ms: Some(hit.ts_ms),
                        folder_name: None,
                    });
                }
            }
        }
        "folder" => {
            add_global_hits(store, trimmed, folder_id, &mut citations)?;
        }
        _ => {
            add_global_hits(store, trimmed, None, &mut citations)?;
        }
    }

    let answer = if citations.is_empty() {
        "No grounded answer found in local transcripts or notes for that question.".to_string()
    } else {
        let lead = citations
            .iter()
            .take(4)
            .map(|citation| format!("- {}", citation.excerpt))
            .collect::<Vec<_>>()
            .join("\n");
        format!(
            "Grounded answer for \"{}\":\n\n{}\n\nAudire found {} supporting source(s).",
            trimmed,
            lead,
            citations.len()
        )
    };

    Ok(AskAudireResp { answer, citations })
}

fn add_global_hits(
    store: &LocalStore,
    query: &str,
    folder_id: Option<i64>,
    citations: &mut Vec<AskCitation>,
) -> Result<()> {
    let segment_hits = store.search_segments_global(&keyword_query(query), 6, folder_id)?;
    let structured_hits = store.search_structured_items_global(query, 4, folder_id)?;
    let note_hits = store.search_standalone_notes_global(query, 3, folder_id)?;

    citations.extend(segment_hits.into_iter().map(map_segment_hit));
    citations.extend(structured_hits.into_iter().map(map_structured_hit));
    citations.extend(note_hits.into_iter().map(map_note_hit));

    Ok(())
}

fn map_segment_hit(hit: SegmentSearchHit) -> AskCitation {
    AskCitation {
        kind: "segment".to_string(),
        meeting_id: Some(hit.meeting_id),
        note_id: None,
        item_id: None,
        segment_id: Some(hit.segment.id),
        title: hit.meeting_title.unwrap_or_else(|| "Meeting".to_string()),
        excerpt: hit.segment.text,
        ts_ms: Some(hit.segment.ts_ms),
        folder_name: hit.folder_name,
    }
}

fn map_structured_hit(hit: StructuredItemSearchHit) -> AskCitation {
    AskCitation {
        kind: "structured_item".to_string(),
        meeting_id: Some(hit.meeting_id),
        note_id: None,
        item_id: Some(hit.item_id),
        segment_id: None,
        title: hit.meeting_title.unwrap_or_else(|| "Meeting".to_string()),
        excerpt: hit.text,
        ts_ms: None,
        folder_name: hit.folder_name,
    }
}

fn map_note_hit(hit: StandaloneNoteSearchHit) -> AskCitation {
    AskCitation {
        kind: "standalone_note".to_string(),
        meeting_id: None,
        note_id: Some(hit.note_id),
        item_id: None,
        segment_id: None,
        title: hit.title,
        excerpt: hit.text,
        ts_ms: None,
        folder_name: hit.folder_name,
    }
}

fn keyword_query(text: &str) -> String {
    text.split(|c: char| !c.is_alphanumeric())
        .map(|part| part.trim().to_lowercase())
        .filter(|part| part.len() > 2)
        .take(5)
        .collect::<Vec<_>>()
        .join(" OR ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::db::LocalStore;

    #[test]
    fn ask_returns_grounded_citations() {
        let store = LocalStore::open_memory().unwrap();
        let meeting_id = store.create_meeting("mock").unwrap();
        let folder = store
            .create_folder("Roadmap", "project", None, "local_only", None, None)
            .unwrap();
        store
            .assign_meeting_folder(&meeting_id, Some(folder.id))
            .unwrap();
        store
            .insert_segment(
                &meeting_id,
                "SYS",
                1000,
                "We decided to delay launch until legal approves the contract.",
                Some(0.9),
            )
            .unwrap();

        let resp = ask(
            &store,
            "What was decided about launch?",
            "folder",
            None,
            Some(folder.id),
        )
        .unwrap();
        assert!(!resp.citations.is_empty());
        assert!(resp.answer.contains("Grounded answer"));
    }
}
