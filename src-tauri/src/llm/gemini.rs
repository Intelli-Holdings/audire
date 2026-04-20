// LLM gateway: Google Gemini.
//
// Uses the Generative Language API with X-goog-api-key header.
// Model: gemini-flash-latest (fast, cost-effective)

use async_trait::async_trait;

use crate::keyvault::vault::KeyVault;
use super::provider::LlmProvider;

pub struct GeminiProvider;

#[async_trait]
impl LlmProvider for GeminiProvider {
    fn name(&self) -> &str {
        "Google Gemini"
    }
    fn id(&self) -> &str {
        "gemini"
    }
    fn is_available(&self, keyvault: &KeyVault) -> bool {
        keyvault.has_provider_key("gemini")
    }
    async fn call(
        &self,
        keyvault: &KeyVault,
        system_prompt: &str,
        user_prompt: &str,
    ) -> Result<String, String> {
        let key = keyvault
            .get_provider_key("gemini")
            .ok_or_else(|| "Gemini API key not configured".to_string())?;
        call_generate_content(&key, system_prompt, user_prompt)
            .await
            .map_err(|e| e.to_string())
    }
}

async fn call_generate_content(
    api_key: &str,
    system_prompt: &str,
    user_prompt: &str,
) -> anyhow::Result<String> {
    let client = reqwest::Client::new();

    let body = serde_json::json!({
        "system_instruction": {
            "parts": [{ "text": system_prompt }]
        },
        "contents": [{
            "parts": [{ "text": user_prompt }]
        }],
        "generationConfig": {
            "maxOutputTokens": 2048,
            "temperature": 0.3
        }
    });

    let resp = client
        .post("https://generativelanguage.googleapis.com/v1beta/models/gemini-2.0-flash:generateContent")
        .header("X-goog-api-key", api_key)
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await?
        .error_for_status()?;

    let json: serde_json::Value = resp.json().await?;

    let text = json["candidates"]
        .get(0)
        .and_then(|c| c["content"]["parts"].get(0))
        .and_then(|p| p["text"].as_str())
        .unwrap_or("")
        .to_string();

    if text.is_empty() {
        anyhow::bail!("Gemini returned an empty response");
    }

    Ok(text)
}
