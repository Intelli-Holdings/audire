pub mod recipe;

#[cfg(feature = "llm_openai")]
pub mod openai;

#[cfg(feature = "llm_anthropic")]
pub mod anthropic;

use crate::keyvault::vault::KeyVault;

/// Try to call an LLM with fallback chain: Anthropic first, then OpenAI.
/// Returns Ok(response) if any provider succeeds, or Err if none available.
pub async fn llm_call(
    keyvault: &KeyVault,
    system_prompt: &str,
    user_prompt: &str,
) -> Result<String, String> {
    // Try Anthropic first
    #[cfg(feature = "llm_anthropic")]
    {
        if let Some(key) = keyvault.get_provider_key("anthropic") {
            match anthropic::call_messages(&key, system_prompt, user_prompt).await {
                Ok(text) => return Ok(text),
                Err(e) => {
                    eprintln!("Anthropic LLM call failed, trying OpenAI: {}", e);
                }
            }
        }
    }

    // Try OpenAI second
    #[cfg(feature = "llm_openai")]
    {
        if let Some(key) = keyvault.get_provider_key("openai") {
            match openai::call_chat_completion(&key, system_prompt, user_prompt).await {
                Ok(text) => return Ok(text),
                Err(e) => {
                    eprintln!("OpenAI LLM call failed: {}", e);
                }
            }
        }
    }

    Err("No LLM API key configured. Add an Anthropic or OpenAI key in Settings.".to_string())
}

/// Check if any LLM provider key is available.
pub fn has_any_llm_key(keyvault: &KeyVault) -> bool {
    #[cfg(feature = "llm_anthropic")]
    {
        if keyvault.has_provider_key("anthropic") {
            return true;
        }
    }
    #[cfg(feature = "llm_openai")]
    {
        if keyvault.has_provider_key("openai") {
            return true;
        }
    }
    false
}
