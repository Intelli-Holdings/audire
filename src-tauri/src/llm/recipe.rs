use crate::error::{ParaError, Result};
use crate::state::AppState;

/// "Recipes" are post-transcription transforms.
///
/// MVP: retrieval-first local summary that combines user notes with FTS5-retrieved
/// transcript segments. No cloud LLM call unless feature-gated AND BYOK key present.
///
/// Privacy: BYOK required for any cloud call.
pub fn run_recipe(state: &AppState, meeting_id: &str, recipe_id: &str) -> Result<String> {
    if recipe_id != "summary" {
        return Err(ParaError::InvalidState(format!(
            "unknown recipe_id: {}",
            recipe_id
        )));
    }

    let notes = state.store.notes_for_meeting(meeting_id)?;

    let retrieved = state
        .store
        .top_segments_for_query(meeting_id, "decision OR action OR next OR important", 8)?;

    let mut out = String::new();
    out.push_str("# Summary (local retrieval)\n\n");

    out.push_str("## Your notes\n");
    if notes.is_empty() {
        out.push_str("- (no notes taken)\n");
    } else {
        for n in &notes {
            out.push_str("- ");
            out.push_str(n);
            out.push('\n');
        }
    }

    out.push_str("\n## Retrieved transcript highlights (FTS5/BM25)\n");
    if retrieved.is_empty() {
        out.push_str("- (no matching segments found)\n");
    } else {
        for t in &retrieved {
            out.push_str("- ");
            out.push_str(t);
            out.push('\n');
        }
    }

    #[cfg(feature = "llm_openai")]
    {
        if let Some(_key) = state.keyvault.get_provider_key("openai") {
            out.push_str("\n## LLM (OpenAI)\n");
            out.push_str("Feature enabled; BYOK key found. TODO: implement in llm/openai.rs\n");
        }
    }

    #[cfg(feature = "llm_anthropic")]
    {
        if let Some(_key) = state.keyvault.get_provider_key("anthropic") {
            out.push_str("\n## LLM (Anthropic)\n");
            out.push_str(
                "Feature enabled; BYOK key found. TODO: implement in llm/anthropic.rs\n",
            );
        }
    }

    Ok(out)
}
