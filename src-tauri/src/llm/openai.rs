// Optional LLM gateway: OpenAI (feature = llm_openai).
//
// BYOK: key fetched from KeyVault in Rust core only.
// Do not log prompts or secrets in production builds.

#[cfg(feature = "llm_openai")]
pub async fn call_chat_completion(_api_key: &str, _prompt: &str) -> anyhow::Result<String> {
    // TODO: implement using reqwest + OpenAI chat completions API.
    // - POST https://api.openai.com/v1/chat/completions
    // - Authorization: Bearer {api_key}
    // - Model: gpt-4o (or user-configurable)
    // - Stream response for better UX
    anyhow::bail!("OpenAI gateway not implemented in MVP scaffold. Enable llm_openai feature and implement.")
}
