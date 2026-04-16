// Optional LLM gateway: Anthropic (feature = llm_anthropic).
//
// BYOK: key fetched from KeyVault in Rust core only.
// Do not log prompts or secrets in production builds.

#[cfg(feature = "llm_anthropic")]
pub async fn call_messages(_api_key: &str, _prompt: &str) -> anyhow::Result<String> {
    // TODO: implement using reqwest + Anthropic Messages API.
    // - POST https://api.anthropic.com/v1/messages
    // - x-api-key: {api_key}
    // - anthropic-version: 2023-06-01
    // - Model: claude-sonnet-4-20250514 (or user-configurable)
    // - Stream response for better UX
    anyhow::bail!("Anthropic gateway not implemented in MVP scaffold. Enable llm_anthropic feature and implement.")
}
