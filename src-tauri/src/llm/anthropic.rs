// LLM gateway: Anthropic (feature = llm_anthropic).
//
// BYOK: key fetched from KeyVault in Rust core only.
// Do not log prompts or secrets in production builds.

#[cfg(feature = "llm_anthropic")]
pub async fn call_messages(
    api_key: &str,
    system_prompt: &str,
    user_prompt: &str,
) -> anyhow::Result<String> {
    let client = reqwest::Client::new();

    let body = serde_json::json!({
        "model": "claude-sonnet-4-20250514",
        "max_tokens": 2048,
        "system": system_prompt,
        "messages": [
            { "role": "user", "content": user_prompt },
        ],
    });

    let resp = client
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await?
        .error_for_status()?;

    let json: serde_json::Value = resp.json().await?;

    let text = json["content"]
        .as_array()
        .and_then(|arr| arr.first())
        .and_then(|block| block["text"].as_str())
        .unwrap_or("")
        .to_string();

    if text.is_empty() {
        anyhow::bail!("Anthropic returned an empty response");
    }

    Ok(text)
}
