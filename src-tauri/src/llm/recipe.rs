use crate::error::{ParaError, Result};
use crate::services::meeting_notes;
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

    let note = match state.store.get_structured_meeting_note(meeting_id)? {
        Some(note) => note,
        None => meeting_notes::generate_and_store(&state.store, meeting_id, None)?,
    };

    let mut out = meeting_notes::render_markdown(&note);

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
            out.push_str("Feature enabled; BYOK key found. TODO: implement in llm/anthropic.rs\n");
        }
    }

    Ok(out)
}
