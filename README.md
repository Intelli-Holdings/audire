# Audire

Audire is a **local-first, privacy-by-default** desktop app for real-time meeting transcription, notes, and AI-powered analysis. Built with Tauri v2 (Rust backend + system WebView UI) for macOS, Windows, and Linux.

It "listens alongside you" — capturing microphone + system audio, performing real-time transcription via streaming ASR providers (Deepgram Flux v2 / AssemblyAI Universal-3 Pro), and storing transcripts + notes locally in an encrypted database. Optionally, post-processing "Recipes" powered by cloud LLMs (OpenAI / Anthropic) generate summaries, action items, follow-up emails, and more.

**Desktop-only.** No web deployment. No PWA. No browser target. Tauri builds a native installer/bundle.

---

## Features

### Real-time Transcription
- Live streaming transcription with partial and final results
- Three ASR providers: **Deepgram Flux v2**, **AssemblyAI Universal-3 Pro**, and **Mock** (offline testing)
- Real-time audio level visualization
- Capture elapsed timer
- Floating transcript card with collapse/settings controls

### Audio Capture
- **System audio** — captures speaker output (meeting audio from Google Meet, Teams, Zoom, etc.)
- **Microphone** — captures your voice via cpal (cross-platform)
- **Target process** — capture audio from a specific application (Windows/macOS)
- Audio resampling (48kHz/44.1kHz to 16kHz mono) for ASR optimization
- Memory-only ring buffers — **no audio is ever written to disk**

### Meeting Notes
- Free-form note-taking during live capture
- **6 meeting templates** for structured note generation:
  - Generic, Sales Call, 1:1, Standup, Interview, Client Review
- Auto-generated structured sections: Key Decisions, Action Items, Open Questions, Risks/Blockers, Quotes/Highlights
- Editable summary and individual items
- Citation linking to transcript timestamps
- Markdown export

### Standalone Notes
- Create independent notes outside of meetings
- Title + text editing with 3-second auto-save
- Folder organization
- Full-text search indexing

### Folders & Organization
- Create private folders with name, color, and description
- Assign meetings and standalone notes to folders
- Folder navigation in sidebar
- Folder-scoped search

### AI-Powered Recipes (BYOK LLM Keys)

**Single-meeting recipes:**
- Summary — meeting overview (rule-based fallback if no LLM key)
- Action Items — extracted tasks with owners and deadlines
- Follow-up Email — draft email from meeting content
- Key Decisions — decisions with surrounding context

**Cross-meeting recipes:**
- Recent Todos — outstanding to-dos across your last 10 meetings
- Weekly Recap — themes, progress, and blockers for the week

**LLM provider fallback:** Anthropic (Claude) > OpenAI > Rule-based > Error

### Ask Audire (Search)
- Rule-based keyword search across all meetings, notes, and structured items
- LLM-powered semantic search (requires API key)
- Scoped search: all content, single meeting, or folder
- Results include citations with source type, excerpts, and timestamps

### Calendar Integration
- **Google Calendar** and **Microsoft Outlook** support
- OAuth with PKCE flow, localhost callback on port 8848
- Upcoming events displayed on Home view, grouped by date
- Calendar connection status with connected email display
- Multi-provider support (connect Google + Microsoft simultaneously)

### Chat Interface
- Dedicated chat view for querying your meeting data
- Scope dropdown (My notes / All meetings)
- **Voice input** via Web Speech API — speak your queries
- Recipe shortcut chips (Recent Todos, Coach Me, Weekly Recap, etc.)
- Recent meetings quick-access list

### People & Companies
- **People** — table of meeting participants with avatars, last note date, note count
- **Companies** — organization management with domain, people count
- Manual addition via inline forms
- Auto-extracted from meeting transcripts

### Settings
- **Preferences** — live meeting indicator, open on login, move aside when idle, dark/light theme
- **Calendar** — Google/Microsoft OAuth configuration (Client ID, Client Secret, Tenant ID)
- **API Keys** — save/delete keys for Deepgram, AssemblyAI, OpenAI, Anthropic with status indicators
- **About** — product info and privacy statement

### UI & Navigation
- Collapsible sidebar with state persistence
- Back/forward navigation with history
- Global search (Ctrl+K)
- Skeleton loaders during data fetching
- Toast notifications (success, error, info)
- Titlebar with quick-note button and window controls
- Folder creation modal

---

## Privacy-by-default (Enforced)

- **Audio is never persisted, anywhere — not on the recorder's device, not in the cloud, not ever.** Audio exists only in RAM during capture (a small ring buffer feeds the streaming ASR socket) and is discarded as soon as it's been transcribed. The schema in `src-tauri/src/store/db.rs` intentionally has no `audio_bytes`, `audio_blob`, or attachment column — there is no place to put audio even if a future caller tried.
- **Only text transcripts + your manual notes are persisted**, and only on your device.
- **Local DB is encrypted at rest** (SQLCipher via rusqlite `bundled-sqlcipher-vendored-openssl`). The DB key lives in your OS keyring, never on disk in plaintext, and is never returned to the WebView.
- **BYOK**: Cloud calls (ASR / LLM) require keys you provide. Keys are read in Rust core only, fetched from environment variables or the OS keyring; there is no IPC command that returns a secret to the frontend.
- **TLS everywhere**: ASR streaming uses `wss://` (Deepgram, AssemblyAI). LLM calls use `https://`. No plaintext audio or text leaves the device.
- **No telemetry**, no analytics, no crash reporters phoning home. The app makes outbound network calls only to the providers you've configured.
- **Audire Sync (optional, opt-in)**: when you sign in to the optional cloud sync service, transcripts and notes are end-to-end encrypted on your device before upload. Audio is still never synced — there is nothing to sync, because nothing was kept.

---

## Developer Quickstart

### Prerequisites
- [Node.js / npm](https://nodejs.org/) (package manager & JS runtime)
- Rust stable toolchain (1.77+)
- OS audio permissions (see platform-specific notes below)
- On Linux: `libasound2-dev` (ALSA headers for cpal)
- On Windows: Strawberry Perl (for `bundled-sqlcipher-vendored-openssl`)

### Install
```bash
npm install
```

### Run (dev)
```bash
npm run tauri dev
```

### Build (release)
```bash
npm run tauri build
```

### Run tests
```bash
cd src-tauri && cargo test
```

---

## Setting API Keys (BYOK)

Copy `.env.example` to `.env` and fill in your keys:

```bash
cp .env.example .env
# Edit .env with your keys
```

Or set environment variables directly:

```powershell
# Windows PowerShell
$env:AUDIRE_DEEPGRAM_API_KEY="your-key-here"
$env:AUDIRE_ASSEMBLYAI_API_KEY="your-key-here"
# Optional LLM keys
$env:AUDIRE_OPENAI_API_KEY="your-key-here"
$env:AUDIRE_ANTHROPIC_API_KEY="your-key-here"
```

```bash
# macOS/Linux
export AUDIRE_DEEPGRAM_API_KEY="your-key-here"
export AUDIRE_ASSEMBLYAI_API_KEY="your-key-here"
```

Or store in OS keyring using the included keytool:
```bash
cargo run -p audire --bin audire_keytool -- set deepgram YOUR_KEY
cargo run -p audire --bin audire_keytool -- set assemblyai YOUR_KEY
cargo run -p audire --bin audire_keytool -- set dbkey RANDOM_32B_HEX_OR_PASSPHRASE
```

Or use the **in-app Settings UI**: click your user profile in the sidebar to open Settings, where you can save/delete API keys per provider. Keys are stored in the OS keyring (macOS Keychain / Windows Credential Manager / Linux Secret Service).

**Notes:**
- Keys are fetched by the Rust core only.
- No `get_key` IPC exists. Keys never leave the native layer.

---

## ASR Providers

### Deepgram Flux v2
- Endpoint: `wss://api.deepgram.com/v2/listen`
- Model: `flux-general-en`
- Events: TurnInfo with `StartOfTurn`, `Update`, `EagerEndOfTurn`, `TurnResumed`, `EndOfTurn`
- Close: `{"type":"CloseStream"}`
- Reference: https://developers.deepgram.com/reference/speech-to-text/listen-flux

### AssemblyAI Universal-3 Pro Streaming
- Endpoint: `wss://streaming.assemblyai.com/v3/ws?speech_model=u3-rt-pro`
- Events: `Begin`, `Turn` (with `end_of_turn` boolean), `Termination`
- Close: `{"type":"ForceEndpoint"}` then `{"type":"Terminate"}`
- Reference: https://www.assemblyai.com/docs/api-reference/streaming-api/universal-3-pro-streaming/universal-3-pro-streaming

### Mock Provider (Offline Testing)
- Select "Mock (offline test)" in the UI to test without network/keys
- Streams canned partial/final transcript events on a timer

---

## OS Support

| Feature | Windows | macOS | Linux |
|---------|---------|-------|-------|
| Mic capture | cpal | cpal | cpal |
| System audio | WASAPI loopback | ScreenCaptureKit (helper) | PulseAudio/PipeWire monitor |

### Windows
- System audio captured via WASAPI loopback from the default render endpoint
- Captures all system output ("what you hear") including Meet in Chrome/Edge + Teams app audio
- Per-process loopback (Windows 10 20348+ / Windows 11) available as advanced option

### macOS
- System audio captured via ScreenCaptureKit helper binary
- **Requires Screen Recording permission**:
  1. Open **System Settings** > **Privacy & Security** > **Screen Recording**
  2. Enable Audire (or the Terminal if running in dev mode)
  3. You may need to restart Audire after granting permission
- If the helper binary is missing or permission is denied, Audire will show a clear error message

### Linux
- System audio captured from PulseAudio/PipeWire monitor sources
- The app auto-detects monitor/loopback devices via cpal
- If no monitor source is found, the UI shows guidance:
  1. Ensure PulseAudio or PipeWire-pulse is running
  2. Run `pactl list short sources` to verify a `.monitor` source exists
  3. If using PipeWire without PulseAudio compat, configure a loopback device

---

## Memory & Buffer Budgets

| Buffer | Size |
|--------|------|
| Mic ring (5s @ 48kHz stereo 16-bit) | ~0.96 MB |
| System ring (5s @ 48kHz stereo 16-bit) | ~0.96 MB |

Tokio runtime: `worker_threads=2`, `thread_stack_size=512 KiB`.

Audio frame dropping policy: if send queue is full, drop oldest frame and continue (prefer continuity over RAM growth).

---

## Installer Size

Target: **<= 123 MB** for the Windows installer (.msi/.exe).

Configured with WebView2 `downloadBootstrapper` mode (tiny ~1.8 MB bootstrapper downloads WebView2 at install time if needed). DO NOT use `offlineInstaller` or `fixedVersion`.

Check artifact size locally after build:
```powershell
# PowerShell
.\scripts\check-size.ps1

# Bash (Git Bash / WSL / macOS / Linux)
bash scripts/check-size.sh
```

CI enforces this limit on every push via `.github/workflows/ci-windows-size.yml`.

---

## Architecture

```
index.html + src/main.js + src/style.css  (Tauri WebView UI)
        |
        | Tauri IPC (commands + events)
        |
src-tauri/src/
  lib.rs            -- Tauri app setup
  main.rs           -- entry point
  error.rs          -- AudireError types
  state.rs          -- AppState (store, keyvault, tokio runtime)
  ipc.rs            -- Tauri commands (40+ IPC handlers)
  audio/
    mod.rs          -- module declarations
    types.rs        -- PcmFormat, AudioChunk
    ring.rs         -- ringbuf ring buffer creation
    resample.rs     -- mono mix, downsample 48k/44.1k->16k, frame 100ms
    mic_cpal.rs     -- mic capture via cpal
    system_capture.rs -- unified system capture dispatcher
    system_windows_wasapi.rs -- WASAPI loopback (Windows)
    system_macos_sck.rs      -- ScreenCaptureKit helper (macOS)
    system_linux_monitor.rs  -- PulseAudio/PipeWire monitor (Linux)
  asr/
    mod.rs          -- pipeline orchestration (capture + ASR + store)
    events.rs       -- unified ASR event parsing + tests
    deepgram.rs     -- Deepgram Flux v2 WebSocket client
    assemblyai.rs   -- AssemblyAI U3 Pro WebSocket client
    mock.rs         -- mock provider for offline testing
  store/
    mod.rs          -- module export
    db.rs           -- SQLCipher local store + FTS5 + participants/orgs/standalone_notes + tests
  services/
    calendar.rs     -- Google/Microsoft OAuth + event fetching
  keyvault/
    mod.rs          -- module export
    vault.rs        -- OS keyring + env var key vault
  llm/
    mod.rs          -- module declarations
    recipe.rs       -- post-transcription recipes (summary, action items, etc.)
    openai.rs       -- optional OpenAI gateway
    anthropic.rs    -- optional Anthropic gateway
  bin/
    audire_keytool.rs -- CLI for storing keys in OS keyring

src/views/
  home.js           -- Home view (calendar events + meeting notes)
  chat.js           -- Chat view (AI chat + voice input + recipes)
  transcript.js     -- Live transcription + meeting detail view
  notes.js          -- Note editor (two-column layout)
  mynotes.js        -- My Notes (private notes + folders)
  settings.js       -- Settings (preferences, calendar, API keys, about)
  people.js         -- People management
  companies.js      -- Company management
  shared.js         -- Shared with me (placeholder)
```

### macOS helper
```
src-tauri/helpers/audire_sck_helper.swift
```

---

## End-to-End Testing Checklist

1. **Build and run**: `npm run tauri dev`
2. **Start capture (mock)**: Select "Mock (offline test)" provider, click Start. Verify partial/final transcript updates appear.
3. **Start capture (real)**: Set API keys, select Deepgram or AssemblyAI, click Start. Play audio through a meeting call.
4. **Verify partial updates**: Transcript panel shows interim text updating in real time.
5. **Stop capture**: Click Stop. Verify status shows "finalizing" then "stopped", meeting notes are generated.
6. **Export Markdown**: Click "Export MD". Verify a `.md` file is created.
7. **Meeting templates**: Select a template (e.g., Sales Call). Generate structured notes and verify sections match template type.
8. **Recipe execution**: Run a recipe (e.g., Summary). Verify output is generated.
9. **Calendar events**: Connect Google or Microsoft calendar. Verify upcoming events display on Home view grouped by date.
10. **Chat + voice input**: Open Chat view. Click the mic icon and speak a query. Verify voice input works.
11. **Ask Audire**: Use the quick-ask bar on Home or the Chat view. Verify search results include citations.
12. **No audio on disk**: Verify no `.wav`, `.pcm`, or audio files are created anywhere.
13. **Keys not leaked**: Verify no API keys appear in console logs or network events visible to the WebView.
14. **Settings view**: Open Settings. Save/delete an API key. Connect/disconnect a calendar provider.
15. **Notes view**: Navigate to My Notes. Create a note, edit it, verify auto-save.
16. **Folders**: Create a folder, assign a note to it, verify folder navigation.
17. **People view**: Navigate to People. Verify participants render. Add a person.
18. **Companies view**: Navigate to Companies. Verify orgs render. Add a company.

## License

Copyright 2026 Intelli Holdings Inc. All rights reserved.
