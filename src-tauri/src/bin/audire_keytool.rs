//! audire_keytool — CLI tool to store BYOK secrets in the OS keyring.
//!
//! Usage:
//!   audire_keytool set <provider> <key>
//!
//! Providers: deepgram | assemblyai | openai | anthropic | dbkey
//!
//! Notes:
//! - This tool writes secrets to the OS keyring (Keychain / Credential Manager / Secret Service).
//! - The main Audire app reads secrets from the keyring but NEVER exposes them to the WebView.
//! - There is no IPC command to retrieve keys.

use audire::keyvault::vault::KeyVault;

fn usage() {
    eprintln!("Usage:");
    eprintln!("  audire_keytool set <provider> <key>");
    eprintln!();
    eprintln!("Providers: deepgram | assemblyai | openai | anthropic | dbkey");
    eprintln!();
    eprintln!("Example:");
    eprintln!("  audire_keytool set deepgram dg-xxxxxxxxxxxx");
    eprintln!("  audire_keytool set dbkey my-encryption-passphrase");
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 {
        usage();
        std::process::exit(2);
    }

    let vault = KeyVault::new("audire");

    match args[1].as_str() {
        "set" => {
            if args.len() != 4 {
                usage();
                std::process::exit(2);
            }
            let provider = &args[2];
            let key = &args[3];
            vault.set_provider_key(provider, key)?;
            println!("Stored key for '{}' in OS keyring.", provider);
        }
        _ => {
            usage();
            std::process::exit(2);
        }
    }

    Ok(())
}
