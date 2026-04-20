// LLM gateway: OpenAI (feature = llm_openai).
//
// BYOK: key fetched from KeyVault in Rust core only.
// Do not log prompts or secrets in production builds.

#[cfg(feature = "llm_openai")]
pub async fn call_chat_completion(
    api_key: &str,
    system_prompt: &str,
    user_prompt: &str,
) -> anyhow::Result<String> {
    let client = reqwest::Client::new();

    let body = serde_json::json!({
        "model": "gpt-4o",
        "messages": [
            { "role": "system", "content": system_prompt },
            { "role": "user", "content": user_prompt },
        ],
        "max_tokens": 2048,
        "temperature": 0.3,
    });

    let resp = client
        .post("https://api.openai.com/v1/chat/completions")
        .header("Authorization", format!("Bearer {}", api_key))
        .json(&body)
        .send()
        .await?
        .error_for_status()?;

    let json: serde_json::Value = resp.json().await?;

    let text = json["choices"]
        .get(0)
        .and_then(|c| c["message"]["content"].as_str())
        .unwrap_or("")
        .to_string();

    if text.is_empty() {
        anyhow::bail!("OpenAI returned an empty response");
    }

    Ok(text)
}
