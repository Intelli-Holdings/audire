# Contributing to Audire

Thanks for your interest in contributing! This guide covers the development setup and conventions.

## Prerequisites

- [Node.js / npm](https://nodejs.org/) (package manager & JS runtime)
- [Rust](https://rustup.rs/) stable toolchain (1.77+)
- Platform-specific audio dependencies (see README for details)
- On Windows: Strawberry Perl (for `bundled-sqlcipher-vendored-openssl`)

## Setup

```bash
# Clone the repository
git clone https://github.com/Intelli-Holdings/audire.git
cd audire

# Install JS dependencies
npm install

# Run in development mode
npm run tauri dev

# Run Rust tests
cd src-tauri && cargo test
```

## Code Structure

```
index.html              -- App shell (sidebar, modals, capture bar, titlebar)
src/main.js             -- Vanilla JS: navigation, views, Tauri IPC calls
src/views/
  home.js               -- Home view (calendar events + meeting notes)
  chat.js               -- Chat view (AI queries, voice input, recipes)
  transcript.js         -- Live transcription + meeting detail
  notes.js              -- Note editor (two-column layout)
  mynotes.js            -- My Notes (private notes + folders)
  settings.js           -- Settings (preferences, calendar, API keys, about)
  people.js             -- People table + add form
  companies.js          -- Companies table + add form
  shared.js             -- Shared with me (placeholder)
src/style.css           -- Dark theme CSS with design tokens
src-tauri/src/
  lib.rs                -- Tauri app builder + command registration
  ipc.rs                -- All IPC command handlers (40+)
  state.rs              -- Shared AppState
  error.rs              -- AudireError enum
  audio/                -- Mic + system audio capture per platform
  asr/                  -- Streaming ASR provider clients
  store/db.rs           -- SQLCipher DB, migrations, queries
  services/calendar.rs  -- Google/Microsoft OAuth + calendar events
  keyvault/vault.rs     -- OS keyring + env var key management
  llm/                  -- LLM recipe runners (summary, action items, etc.)
```

## Coding Conventions

### Frontend (vanilla JS)
- No frameworks — plain DOM manipulation with `document.getElementById` / `innerHTML`
- Each view is a separate ES module file in `src/views/`
- Views export a `render*()` function that returns HTML or sets `content.innerHTML`
- Use `invoke()` from `@tauri-apps/api/core` for all backend calls
- Always escape user content with `escapeHtml()` before inserting into HTML
- Use async/await; handle errors with try/catch
- Custom CSS with design tokens (no Tailwind or CSS frameworks)

### Rust
- Error handling: return `crate::error::Result<T>`, map errors with `.map_err(|e| AudireError::Db(e.to_string()))`
- Serializable structs for IPC responses: derive `Serialize`
- Database access: lock `self.inner.lock().unwrap()`, prepare statement, query_map, collect
- Security: never return secrets via IPC, never log raw API keys
- IPC command names must exactly match Rust function names (Tauri v2 convention)

## How to Add a New IPC Command

1. Add a method to `LocalStore` in `src-tauri/src/store/db.rs` (if it touches the DB)
2. Add a `#[tauri::command]` function in `src-tauri/src/ipc.rs`
3. Register it in `tauri::generate_handler![]` in `src-tauri/src/lib.rs`
4. Call it from JS: `await invoke('command_name', { arg1, arg2 })`

## How to Add a New View

1. Create a new file `src/views/viewname.js` with a `renderViewName()` function
2. Import and add a `case 'view-name':` to the `renderView()` switch in `src/main.js`
3. Add any needed CSS classes to `src/style.css`
4. If the view needs a sidebar link, add a `<a data-view="view-name">` in `index.html`

## How to Add a New Recipe

1. Define the recipe in `src-tauri/src/llm/recipe.rs`
2. Add the recipe ID to the match statement in the recipe runner
3. Add a chip/button in the relevant view (Chat or Transcript)
4. Call via `invoke('run_recipe', { meetingId, recipeId })`

## Testing

- Rust unit tests: `cd src-tauri && cargo test`
- Frontend: manual testing via `npm run tauri dev`
- Use the Mock ASR provider for offline testing (no API keys needed)
- See the end-to-end testing checklist in README.md

## Pull Request Process

1. Create a feature branch from `main`
2. Make your changes, following the conventions above
3. Run `cargo test` and verify the UI manually
4. Open a PR with a clear description of what changed and why
5. Ensure no secrets are committed (check `.gitignore`)
