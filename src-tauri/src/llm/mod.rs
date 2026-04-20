pub mod provider;
pub mod gemini;
pub mod ollama;
pub mod recipe;

#[cfg(feature = "llm_openai")]
pub mod openai;

#[cfg(feature = "llm_anthropic")]
pub mod anthropic;

use crate::keyvault::vault::KeyVault;
use crate::store::db::LocalStore;
use provider::{LlmProvider, LlmRegistry};

/// Build the LLM registry with all compiled providers.
pub fn build_registry(store: LocalStore) -> LlmRegistry {
    let mut providers: Vec<Box<dyn LlmProvider>> = Vec::new();

    #[cfg(feature = "llm_anthropic")]
    {
        providers.push(Box::new(provider::AnthropicProvider));
    }

    #[cfg(feature = "llm_openai")]
    {
        providers.push(Box::new(provider::OpenAiProvider));
    }

    providers.push(Box::new(gemini::GeminiProvider));
    providers.push(Box::new(ollama::OllamaProvider::new(store)));

    LlmRegistry::new(providers)
}

/// Try to call an LLM with fallback chain: Anthropic first, then OpenAI.
/// Returns Ok(response) if any provider succeeds, or Err if none available.
///
/// DEPRECATED: Use `LlmRegistry::call_preferred()` instead for new code.
/// Kept for backward compat with recipe system during transition.
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
    if keyvault.has_provider_key("gemini") {
        return true;
    }
    // Ollama is always available (local, no API key required)
    true
}
