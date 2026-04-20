// LLM gateway: Ollama (local inference).
//
// Uses the /api/chat endpoint with configurable model and endpoint.
// No API key needed — availability is based on endpoint reachability.

use async_trait::async_trait;

use crate::keyvault::vault::KeyVault;
use crate::store::db::LocalStore;
use super::provider::LlmProvider;

pub struct OllamaProvider {
    store: LocalStore,
}

impl OllamaProvider {
    pub fn new(store: LocalStore) -> Self {
        Self { store }
    }
}

#[async_trait]
impl LlmProvider for OllamaProvider {
    fn name(&self) -> &str {
        "Ollama (Local)"
    }
    fn id(&self) -> &str {
        "ollama"
    }
    fn is_available(&self, _keyvault: &KeyVault) -> bool {
        // Ollama is available if the user has configured it (always show as option)
        true
    }
    async fn call(
        &self,
        _keyvault: &KeyVault,
        system_prompt: &str,
        user_prompt: &str,
    ) -> Result<String, String> {
        let settings = self.store.get_detection_settings().map_err(|e| e.to_string())?;
        call_ollama_chat(&settings.ollama_endpoint, &settings.ollama_model, system_prompt, user_prompt)
            .await
            .map_err(|e| e.to_string())
    }
}

async fn call_ollama_chat(
    endpoint: &str,
    model: &str,
    system_prompt: &str,
    user_prompt: &str,
) -> anyhow::Result<String> {
    let client = reqwest::Client::new();

    let url = format!("{}/api/chat", endpoint.trim_end_matches('/'));

    let body = serde_json::json!({
        "model": model,
        "messages": [
            { "role": "system", "content": system_prompt },
            { "role": "user", "content": user_prompt },
        ],
        "stream": false
    });

    let resp = client
        .post(&url)
        .json(&body)
        .send()
        .await?
        .error_for_status()?;

    let json: serde_json::Value = resp.json().await?;

    let text = json["message"]["content"]
        .as_str()
        .unwrap_or("")
        .to_string();

    if text.is_empty() {
        anyhow::bail!("Ollama returned an empty response");
    }

    Ok(text)
}
