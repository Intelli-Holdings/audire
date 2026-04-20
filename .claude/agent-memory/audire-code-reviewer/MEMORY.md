# Audire Codebase Memory

## Architecture
- Tauri v2 desktop app: Rust backend (`src-tauri/src/`) + vanilla JS/HTML/CSS frontend (`src/`, `index.html`)
- No framework (no React/Vue) — pure DOM manipulation in `src/main.js` (~2277 lines)
- Single-file frontend: `src/main.js` + `src/style.css` + `index.html`
- Rust lib entry: `src-tauri/src/lib.rs`, binary entry: `src-tauri/src/main.rs`

## Key Module Map
- `ipc.rs` — all Tauri commands (38 registered, all present in handler chain)
- `state.rs` — AppState: LocalStore + KeyVault + SessionContext + Tokio Runtime + CaptureHandle mutex
- `error.rs` — ParaError enum with thiserror, Serialize impl for Tauri IPC
- `store/db.rs` — SQLCipher LocalStore (~2311 lines), full schema + migrations
- `asr/mod.rs` — audio capture pipeline orchestrator
- `asr/deepgram.rs` — Deepgram Flux v2 WebSocket client
- `asr/assemblyai.rs` — AssemblyAI U3 Pro streaming client
- `asr/events.rs` — unified ASR event parsing + tests
- `asr/mock.rs` — offline testing mock provider
- `audio/mic_cpal.rs` — microphone capture via cpal
- `audio/system_capture.rs` — platform dispatch for system audio
- `audio/system_windows_wasapi.rs` — WASAPI loopback (Windows)
- `audio/resample.rs` — downsampling + framing + tests
- `services/meeting_notes.rs` — structured note generation (rule-based, FTS5 retrieval)
- `services/calendar.rs` — Google + Microsoft OAuth + calendar events
- `services/retrieval.rs` — ask_audire (FTS5-backed keyword search)
- `services/folders.rs` — folder CRUD wrapper
- `services/keys.rs` — key resolution (personal vs. org)
- `llm/recipe.rs` — run_recipe (MVP: calls meeting_notes, TODO stubs for OpenAI/Anthropic)
- `llm/openai.rs` — STUB (not implemented, feature-gated)
- `llm/anthropic.rs` — STUB (not implemented, feature-gated)
- `keyvault/vault.rs` — OS keyring (keyring crate, Windows Credential Manager)

## Database Schema (SQLCipher)
- meetings, segments, notes, export_cache, participants, meeting_participants
- organizations, participant_org, folders, org_shared_keys, session_cache
- calendar_accounts, standalone_notes
- meeting_structured_notes, meeting_note_items, meeting_note_item_citations
- segments_fts (FTS5 virtual table with triggers, best-effort — falls back to LIKE)
- Migration: `CREATE TABLE IF NOT EXISTS` + `ensure_column()` for safe add-column upgrades

## Known Issues (from first audit, 2026-04-17)
- Search modal (Ctrl+K) has NO backend call — searchInput has no event listener, stays static
- Per-process audio capture (WASAPI) is stubbed with hardcoded error
- LLM backends (OpenAI, Anthropic) are stubs — bail! not implemented
- run_recipe only supports recipe_id="summary"; other recipe cards in UI are unconnected
- logo-icon in sidebar shows "P" (placeholder), should show "A" for Audire
- alert() used for export result (lines 612, 615) and recipe gate (2141) — should use showToast
- Hardcoded "Sonnet 4.6" label in chat UI (line 1260) — decorative only, not functional
- FTS5 graceful degradation works (falls back to LIKE) but bm25 ranking lost on fallback
- unwrap() on Mutex::lock() throughout production code — panic if mutex poisoned
- Recipes view: action_items, follow-up email, key decisions, list todos, weekly recap cards
  are UI-only, no backend invocations wired up
- "Shared with me" view is stubbed ("Sharing is coming soon")
- People/Companies filter pills ("People I met", "Everyone") and search button have no handlers
- Folder description field in create-folder modal is not passed to backend
- No auto-save on note editor (only saves on blur)
- calendar.rs: wait_for_oauth_code uses blocking I/O on async context (TcpListener::accept is sync)

## Feature Status Summary
Working: audio capture, ASR (Deepgram+AssemblyAI+mock), structured notes (rule-based),
  transcript storage, meetings CRUD, folders CRUD, standalone notes, participants/orgs,
  calendar OAuth+events, key management (OS keyring), ask_audire (FTS5/LIKE retrieval),
  export to markdown, settings UI

Partial: search modal (UI exists, no backend search), recipe system (only summary works),
  retrieval (keyword only, no vectors/embeddings)

Stubbed: LLM integration (OpenAI/Anthropic feature flags, stubs only), per-process WASAPI
  capture, shared notes, sharing links, multi-device sync, workflow automation

## Dependency Notes
- Tauri v2, cpal 0.15, ringbuf 0.4, rusqlite 0.32 (bundled-sqlcipher-vendored-openssl)
- tokio-tungstenite 0.24 (rustls-tls-webpki-roots), reqwest 0.12 (rustls-tls)
- keyring v3 (windows-native feature), wasapi 0.16 (optional, windows)
- No TypeScript — plain JS frontend, no test framework for frontend
