use crate::error::{ParaError, Result};
use crate::services::meeting_notes;
use crate::state::AppState;

/// Supported recipe IDs.
const VALID_RECIPES: &[&str] = &[
    "summary",
    "action_items",
    "follow_up_email",
    "key_decisions",
    "recent_todos",
    "weekly_recap",
];

/// "Recipes" are post-transcription transforms.
///
/// Each recipe has a system + user prompt template. The LLM fallback chain
/// tries Anthropic first, then OpenAI, then a rule-based fallback for summary
/// (or an error message for recipes that require an LLM).
pub async fn run_recipe(state: &AppState, meeting_id: &str, recipe_id: &str) -> Result<String> {
    if !VALID_RECIPES.contains(&recipe_id) {
        return Err(ParaError::InvalidState(format!(
            "unknown recipe_id: {}",
            recipe_id
        )));
    }

    // Cross-meeting recipes don't need a specific meeting
    if recipe_id == "recent_todos" || recipe_id == "weekly_recap" {
        return run_cross_meeting_recipe(state, recipe_id).await;
    }

    // Single-meeting recipes need transcript context
    let segments = state.store.segments_for_meeting(meeting_id)?;
    let user_notes = state.store.get_notes_for_meeting(meeting_id)?;

    let transcript_text = build_transcript_context(&segments, 4000);
    let notes_text = user_notes
        .iter()
        .map(|n| n.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");

    let context = format!(
        "## Transcript\n{}\n\n## User Notes\n{}",
        if transcript_text.is_empty() {
            "(no transcript available)"
        } else {
            &transcript_text
        },
        if notes_text.is_empty() {
            "(no user notes)"
        } else {
            &notes_text
        }
    );

    let (system_prompt, user_prompt) = prompt_for_recipe(recipe_id, &context);

    // Try LLM via multi-provider registry (preferred → fallback chain)
    match state.llm_registry.call_preferred(&state.keyvault, &state.store, &system_prompt, &user_prompt).await {
        Ok(text) => Ok(text),
        Err(_no_key_msg) => {
            // Rule-based fallback for summary; error for others
            if recipe_id == "summary" {
                let note = match state.store.get_structured_meeting_note(meeting_id)? {
                    Some(note) => note,
                    None => meeting_notes::generate_and_store(
                        &state.store,
                        meeting_id,
                        None,
                    )?,
                };
                let md = meeting_notes::render_markdown(&note);
                Ok(format!(
                    "{}\n\n---\n*Rule-based summary. Add an API key (Anthropic, OpenAI, Gemini) or configure Ollama in Settings for AI-powered results.*",
                    md
                ))
            } else {
                Err(ParaError::Other(format!(
                    "The \"{}\" recipe requires an LLM provider. Add an API key (Anthropic, OpenAI, Gemini) or configure Ollama in Settings.",
                    recipe_id
                )))
            }
        }
    }
}

async fn run_cross_meeting_recipe(state: &AppState, recipe_id: &str) -> Result<String> {
    let meetings = state.store.list_meetings()?;
    let recent = meetings.iter().take(10);

    let mut context_parts = Vec::new();
    for m in recent {
        let title = m.title.as_deref().unwrap_or("Untitled meeting");
        if let Ok(segs) = state.store.segments_for_meeting(&m.id) {
            let excerpt = build_transcript_context(&segs, 600);
            if !excerpt.is_empty() {
                context_parts.push(format!("### {}\n{}", title, excerpt));
            }
        }
    }

    let context = if context_parts.is_empty() {
        "(no recent meeting transcripts available)".to_string()
    } else {
        context_parts.join("\n\n")
    };

    let (system_prompt, user_prompt) = prompt_for_recipe(recipe_id, &context);

    match state.llm_registry.call_preferred(&state.keyvault, &state.store, &system_prompt, &user_prompt).await {
        Ok(text) => Ok(text),
        Err(msg) => Err(ParaError::Other(format!(
            "The \"{}\" recipe requires an LLM provider. {}",
            recipe_id, msg
        ))),
    }
}

fn prompt_for_recipe(recipe_id: &str, context: &str) -> (String, String) {
    let system = match recipe_id {
        "summary" => {
            "You are a meeting notes assistant. Produce a clear, concise summary \
             of the meeting. Include key topics discussed, decisions made, and action items. \
             Use bullet points. Be factual and grounded in the provided transcript."
        }
        "action_items" => {
            "You are a meeting notes assistant. Extract all action items and next steps \
             from the meeting. For each item, identify the owner if mentioned, the task, \
             and any deadline. Format as a numbered list."
        }
        "follow_up_email" => {
            "You are a professional email writer. Draft a follow-up email based on the \
             meeting content. The email should be concise, professional, and reference \
             key decisions and next steps. Use a friendly but business-appropriate tone."
        }
        "key_decisions" => {
            "You are a meeting notes assistant. List all key decisions made during the \
             meeting. For each decision, provide the decision itself and brief context. \
             Format as a numbered list."
        }
        "recent_todos" => {
            "You are a productivity assistant. Review the recent meeting transcripts and \
             extract all outstanding to-do items, action items, and commitments. \
             Group by meeting. Highlight items that appear overdue or urgent."
        }
        "weekly_recap" => {
            "You are a productivity assistant. Generate a weekly status recap from the \
             recent meeting transcripts. Summarize key themes, decisions, progress, \
             and blockers across all meetings. Keep it concise and actionable."
        }
        _ => "You are a helpful assistant.",
    };

    let user = format!(
        "Here is the meeting context:\n\n{}\n\nPlease generate the requested output.",
        context
    );

    (system.to_string(), user)
}

use crate::store::db::SegmentRow;

fn build_transcript_context(segments: &[SegmentRow], max_chars: usize) -> String {
    if segments.is_empty() {
        return String::new();
    }

    // Sample beginning, middle, and end for better coverage
    let total = segments.len();
    let mut selected = Vec::new();

    // Take from beginning
    let begin_count = (total / 3).max(1).min(total);
    selected.extend(segments.iter().take(begin_count));

    // Take from middle
    if total > 2 {
        let mid_start = total / 3;
        let mid_end = (2 * total / 3).min(total);
        let mid_count = ((mid_end - mid_start) / 2).max(1);
        selected.extend(segments[mid_start..mid_end].iter().take(mid_count));
    }

    // Take from end
    if total > 1 {
        let end_count = (total / 3).max(1);
        selected.extend(segments.iter().rev().take(end_count));
    }

    // Deduplicate by id while preserving order
    let mut seen = std::collections::HashSet::new();
    let deduped: Vec<&SegmentRow> = selected
        .into_iter()
        .filter(|s| seen.insert(s.id))
        .collect();

    let mut out = String::new();
    for seg in deduped {
        let line = format!("[{}] {}\n", format_ts(seg.ts_ms), seg.text);
        if out.len() + line.len() > max_chars {
            break;
        }
        out.push_str(&line);
    }

    out
}

fn format_ts(ts_ms: i64) -> String {
    let s = ts_ms / 1000;
    format!("{:02}:{:02}", (s / 60) % 60, s % 60)
}
