// LLM provider abstraction.
//
// Enables runtime dispatch to any configured LLM backend:
// Anthropic, OpenAI, Gemini, or local Ollama.

use async_trait::async_trait;

use crate::keyvault::vault::KeyVault;
use crate::store::db::LocalStore;

/// Unified interface for LLM providers.
#[async_trait]
pub trait LlmProvider: Send + Sync {
    /// Human-readable name (e.g. "Anthropic Claude")
    fn name(&self) -> &str;
    /// Machine identifier (e.g. "anthropic")
    fn id(&self) -> &str;
    /// Whether this provider has a usable key/endpoint configured.
    fn is_available(&self, keyvault: &KeyVault) -> bool;
    /// Call the LLM with a system + user prompt pair.
    async fn call(
        &self,
        keyvault: &KeyVault,
        system_prompt: &str,
        user_prompt: &str,
    ) -> Result<String, String>;
}

/// Registry of all known LLM providers.
/// Maintains a fixed ordering; user selects preferred via settings.
pub struct LlmRegistry {
    providers: Vec<Box<dyn LlmProvider>>,
}

impl LlmRegistry {
    pub fn new(providers: Vec<Box<dyn LlmProvider>>) -> Self {
        Self { providers }
    }

    /// List available provider IDs and names.
    pub fn list(&self, keyvault: &KeyVault) -> Vec<LlmProviderInfo> {
        self.providers
            .iter()
            .map(|p| LlmProviderInfo {
                id: p.id().to_string(),
                name: p.name().to_string(),
                available: p.is_available(keyvault),
            })
            .collect()
    }

    /// Call the user's preferred provider. Falls back through other available providers.
    pub async fn call_preferred(
        &self,
        keyvault: &KeyVault,
        store: &LocalStore,
        system_prompt: &str,
        user_prompt: &str,
    ) -> Result<String, String> {
        let settings = store
            .get_detection_settings()
            .map_err(|e| e.to_string())?;
        let preferred_id = &settings.preferred_llm_provider;

        // Try preferred first
        if let Some(provider) = self.providers.iter().find(|p| p.id() == preferred_id) {
            if provider.is_available(keyvault) {
                match provider.call(keyvault, system_prompt, user_prompt).await {
                    Ok(text) => return Ok(text),
                    Err(e) => {
                        eprintln!(
                            "Preferred LLM provider '{}' failed: {}. Trying fallbacks.",
                            preferred_id, e
                        );
                    }
                }
            }
        }

        // Fallback: try other available providers in order
        for provider in &self.providers {
            if provider.id() == preferred_id {
                continue;
            }
            if provider.is_available(keyvault) {
                match provider.call(keyvault, system_prompt, user_prompt).await {
                    Ok(text) => return Ok(text),
                    Err(e) => {
                        eprintln!("LLM fallback '{}' failed: {}", provider.id(), e);
                    }
                }
            }
        }

        Err("No LLM API key configured. Add an API key in Settings.".to_string())
    }

    /// Test a specific provider by ID.
    pub async fn test_provider(
        &self,
        keyvault: &KeyVault,
        provider_id: &str,
    ) -> Result<String, String> {
        let provider = self
            .providers
            .iter()
            .find(|p| p.id() == provider_id)
            .ok_or_else(|| format!("Unknown provider: {}", provider_id))?;

        if !provider.is_available(keyvault) {
            return Err(format!("Provider '{}' is not configured (missing API key or endpoint).", provider_id));
        }

        provider
            .call(
                keyvault,
                "You are a helpful assistant.",
                "Respond with exactly: OK",
            )
            .await
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct LlmProviderInfo {
    pub id: String,
    pub name: String,
    pub available: bool,
}

// ---- Concrete provider wrappers ----

/// Anthropic Claude provider
#[cfg(feature = "llm_anthropic")]
pub struct AnthropicProvider;

#[cfg(feature = "llm_anthropic")]
#[async_trait]
impl LlmProvider for AnthropicProvider {
    fn name(&self) -> &str {
        "Anthropic Claude"
    }
    fn id(&self) -> &str {
        "anthropic"
    }
    fn is_available(&self, keyvault: &KeyVault) -> bool {
        keyvault.has_provider_key("anthropic")
    }
    async fn call(
        &self,
        keyvault: &KeyVault,
        system_prompt: &str,
        user_prompt: &str,
    ) -> Result<String, String> {
        let key = keyvault
            .get_provider_key("anthropic")
            .ok_or_else(|| "Anthropic API key not configured".to_string())?;
        super::anthropic::call_messages(&key, system_prompt, user_prompt)
            .await
            .map_err(|e| e.to_string())
    }
}

/// OpenAI GPT provider
#[cfg(feature = "llm_openai")]
pub struct OpenAiProvider;

#[cfg(feature = "llm_openai")]
#[async_trait]
impl LlmProvider for OpenAiProvider {
    fn name(&self) -> &str {
        "OpenAI GPT"
    }
    fn id(&self) -> &str {
        "openai"
    }
    fn is_available(&self, keyvault: &KeyVault) -> bool {
        keyvault.has_provider_key("openai")
    }
    async fn call(
        &self,
        keyvault: &KeyVault,
        system_prompt: &str,
        user_prompt: &str,
    ) -> Result<String, String> {
        let key = keyvault
            .get_provider_key("openai")
            .ok_or_else(|| "OpenAI API key not configured".to_string())?;
        super::openai::call_chat_completion(&key, system_prompt, user_prompt)
            .await
            .map_err(|e| e.to_string())
    }
}
