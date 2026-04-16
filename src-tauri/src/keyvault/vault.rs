use keyring::Entry;

/// KeyVault backed by OS credential store:
/// - macOS: Keychain
/// - Windows: Credential Manager
/// - Linux: Secret Service (via D-Bus)
///
/// SECURITY: Keys are never returned to the WebView via IPC.
/// Only the Rust core reads them. There is no `get_key` IPC command.
#[derive(Clone)]
pub struct KeyVault {
    service: String,
}

impl KeyVault {
    pub fn new(service: &str) -> Self {
        Self {
            service: service.to_string(),
        }
    }

    /// Retrieve a provider's API key.
    /// Priority: environment variable → OS keyring.
    pub fn get_provider_key(&self, provider: &str) -> Option<String> {
        // 1. Check environment variables (simplifies MVP; no UI key entry yet).
        let env_key = match provider {
            "deepgram" => std::env::var("PARAAUDIO_DEEPGRAM_API_KEY").ok(),
            "assemblyai" => std::env::var("PARAAUDIO_ASSEMBLYAI_API_KEY").ok(),
            "openai" => std::env::var("PARAAUDIO_OPENAI_API_KEY").ok(),
            "anthropic" => std::env::var("PARAAUDIO_ANTHROPIC_API_KEY").ok(),
            "dbkey" => std::env::var("PARAAUDIO_DB_KEY").ok(),
            _ => None,
        };
        if env_key.is_some() {
            return env_key;
        }

        // 2. Try OS keyring.
        let name = format!("provider:{}", provider);
        let entry = Entry::new(&self.service, &name).ok()?;
        entry.get_password().ok()
    }

    /// Store a provider's API key in the OS keyring.
    pub fn set_provider_key(&self, provider: &str, key: &str) -> anyhow::Result<()> {
        let name = format!("provider:{}", provider);
        let entry = Entry::new(&self.service, &name)?;
        entry.set_password(key)?;
        Ok(())
    }

    /// Check whether a key exists for the given provider (env var or keyring).
    /// Never returns the key itself.
    pub fn has_provider_key(&self, provider: &str) -> bool {
        self.get_provider_key(provider).is_some()
    }

    /// Delete a provider's API key from the OS keyring.
    /// Does not affect environment variables.
    pub fn delete_provider_key(&self, provider: &str) -> anyhow::Result<()> {
        let name = format!("provider:{}", provider);
        let entry = Entry::new(&self.service, &name)?;
        entry.delete_credential()?;
        Ok(())
    }
}
