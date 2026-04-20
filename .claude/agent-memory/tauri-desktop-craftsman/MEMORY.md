# Audire Project Memory

## Project Overview
- Audire: local-first transcription and meeting notes desktop app
- Tauri v2 + vanilla JS frontend (no framework)
- Rust backend with async tokio runtime

## Architecture
- **ASR pipeline**: `src-tauri/src/asr/mod.rs` - audio capture -> resample -> WebSocket -> ASR provider
- **ASR events**: `src-tauri/src/asr/events.rs` - `AsrEvent` enum (DeepgramFlux, AssemblyAi, Mock) with `partial_text()`, `final_text()`, `is_termination()`, `event_type()`
- **ASR providers**: assemblyai, deepgram, mock (in `src-tauri/src/asr/`)
- **Frontend views**: `src/views/transcript.js` - main transcript view with floating recording card
- **IPC events**: `asr:partial`, `asr:final`, `asr:status`, `asr:lifecycle`, `asr:audio_level`
- **State**: `appState` object passed around in frontend; `AppState` with `capture: Mutex<Option<CaptureHandle>>` in Rust

## Key Patterns
- No `log` crate in dependencies - use `eprintln!("[asr] ...")` for diagnostic logging (appears in Tauri dev console stderr)
- Audio pipeline: ring buffers (ringbuf crate) -> mono i16 -> resample to 16kHz -> 100ms frames -> WebSocket binary messages
- Frontend uses `@tauri-apps/api/core` (invoke) and `@tauri-apps/api/event` (listen)
- `escapeHtml()` helper used for safe HTML insertion in transcript.js
- CSS uses custom properties: `--color-*`, `--space-*`, `--text-*`, `--font-*`, `--weight-*`

## Build
- `cargo check` from `src-tauri/` for Rust compilation
- Uses npm (not bun) per commit d183347
- Windows development (MINGW64/Git Bash)

## Dependencies (notable)
- tokio-tungstenite 0.24 with rustls for WebSocket
- cpal 0.15 for mic capture
- wasapi 0.16 (optional, Windows system audio)
- rusqlite with bundled SQLCipher for encrypted local DB
- No log crate, no tauri-plugin-log
