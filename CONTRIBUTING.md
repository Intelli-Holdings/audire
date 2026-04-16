# Contributing to Audire

Thanks for your interest in contributing! This guide covers the development setup and conventions.

## Prerequisites

- [Bun](https://bun.sh/) 1.0+ (package manager & JS runtime)
- [Rust](https://rustup.rs/) stable toolchain (1.77+)
- Platform-specific audio dependencies (see README for details)

## Setup

```bash
# Clone the repository
git clone https://github.com/your-org/audire.git
cd audire

# Install JS dependencies
bun install

# Run in development mode
bun run tauri dev

# Run Rust tests
cd src-tauri && cargo test
```

## Code structure

```
index.html              — App shell (sidebar, modals, capture bar)
src/main.js             — Vanilla JS: navigation, views, Tauri IPC calls
src/style.css           — Dark theme CSS (Granola-inspired)
src-tauri/src/
  lib.rs                — Tauri app builder + command registration
  ipc.rs                — All IPC command handlers
  state.rs              — Shared AppState
  error.rs              — AudireError enum
  audio/                — Mic + system audio capture per platform
  asr/                  — Streaming ASR provider clients
  store/db.rs           — SQLCipher DB, migrations, queries
  keyvault/vault.rs     — OS keyring + env var key management
  llm/                  — Optional LLM recipe runners
```

## Coding conventions

### Frontend (vanilla JS)
- No frameworks — plain DOM manipulation with `document.getElementById` / `innerHTML`
- Each view is a function: `renderViewName()` that sets `content.innerHTML`
- Use `invoke()` from `@tauri-apps/api/core` for all backend calls
- Always escape user content with `escapeHtml()` before inserting into HTML
- Use async/await; handle errors with try/catch

### Rust
- Error handling: return `crate::error::Result<T>`, map errors with `.map_err(|e| ParaError::Db(e.to_string()))`
- Serializable structs for IPC responses: derive `Serialize`
- Database access: lock `self.inner.lock().unwrap()`, prepare statement, query_map, collect
- Security: never return secrets via IPC, never log raw API keys

## How to add a new IPC command

1. Add a method to `LocalStore` in `src-tauri/src/store/db.rs` (if it touches the DB)
2. Add a `#[tauri::command]` function in `src-tauri/src/ipc.rs`
3. Register it in `tauri::generate_handler![]` in `src-tauri/src/lib.rs`
4. Call it from JS: `await invoke('command_name', { arg1, arg2 })`

## How to add a new view

1. Create a `renderViewName()` function in `src/main.js`
2. Add a `case 'view-name':` to the `renderView()` switch
3. Add any needed CSS classes to `src/style.css`
4. If the view needs a sidebar link, add a `<a data-view="view-name">` in `index.html`

## Testing

- Rust unit tests: `cd src-tauri && cargo test`
- Frontend: manual testing via `bun run tauri dev`
- Use the Mock ASR provider for offline testing (no API keys needed)
- See the end-to-end testing checklist in README.md

## Pull request process

1. Create a feature branch from `main`
2. Make your changes, following the conventions above
3. Run `cargo test` and verify the UI manually
4. Open a PR with a clear description of what changed and why
5. Ensure no secrets are committed (check `.gitignore`)
