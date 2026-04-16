# Audire

Audire is a local-first desktop app (macOS, Windows, Linux) built with Tauri v2: Rust native core + system WebView UI.

It "listens alongside you" (microphone + system audio) and performs real-time transcription via streaming ASR providers (Deepgram Flux v2 / AssemblyAI Universal-3 Pro), stores transcripts + notes locally (encrypted), and optionally runs post-processing "Recipes" via cloud LLM gateways (OpenAI / Anthropic) behind feature flags.

**Desktop-only.** No web deployment. No PWA. No browser target. Tauri builds a native installer/bundle.

## UPDATE NOTICE
1. Update your local `.env` file with the new keys
2. The `.env` file is now in `.gitignore` and will not be committed

## Privacy-by-default rules (ENFORCED)
- **No audio is written to disk** at any time (memory-only ring buffers).
- Only text transcripts + your manual notes are persisted.
- Local DB is **encrypted at rest** (SQLCipher via rusqlite `bundled-sqlcipher`).
- **BYOK**: Cloud calls (ASR/LLM) require user-provided keys from environment variables or OS keyring (KeyVault). Keys are **never returned to the WebView**. There is no IPC command to read secrets.
- No logs ever print raw API keys.

## Developer quickstart

### Prerequisites
- [Bun](https://bun.sh/) 1.0+ (package manager & JS runtime)
- Rust stable toolchain (1.77+)
- OS audio permissions (see platform-specific notes below)
- On Linux: `libasound2-dev` (ALSA headers for cpal)

### Install
```bash
bun install
```

### Run (dev)
```bash
bun run tauri dev
```

### Build (release)
```bash
bun run tauri build
```

### Run tests
```bash
cd src-tauri && cargo test
```

## Setting API keys (BYOK)

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
# Optional LLM keys (only if feature flags enabled)
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

Or use the **in-app Settings UI**: click your user profile in the sidebar bottom to open Settings, where you can save/delete API keys per provider. Keys are stored in the OS keyring (macOS Keychain / Windows Credential Manager / Linux Secret Service).

**Notes:**
- Keys are fetched by the Rust core only.
- No `get_key` IPC exists. Keys never leave the native layer.

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

## OS support

| Feature | Windows | macOS | Linux |
|---------|---------|-------|-------|
| Mic capture | cpal | cpal | cpal |
| System audio | WASAPI loopback | ScreenCaptureKit (helper) | PulseAudio/PipeWire monitor |

### Windows
- System audio captured via WASAPI loopback from the default render endpoint.
- Captures all system output ("what you hear") including Meet in Chrome/Edge + Teams app audio.
- Per-process loopback (Windows 10 20348+ / Windows 11) available as advanced option.

### macOS
- System audio captured via ScreenCaptureKit helper binary.
- **Requires Screen Recording permission**:
  1. Open **System Settings** > **Privacy & Security** > **Screen Recording**
  2. Enable Audire (or the Terminal if running in dev mode)
  3. You may need to restart Audire after granting permission
- If the helper binary is missing or permission is denied, Audire will show a clear error message.

### Linux
- System audio captured from PulseAudio/PipeWire monitor sources.
- The app auto-detects monitor/loopback devices via cpal.
- If no monitor source is found, the UI shows guidance:
  1. Ensure PulseAudio or PipeWire-pulse is running
  2. Run `pactl list short sources` to verify a `.monitor` source exists
  3. If using PipeWire without PulseAudio compat, configure a loopback device

## Memory & buffer budgets

| Buffer | Size |
|--------|------|
| Mic ring (5s @ 48kHz stereo 16-bit) | ~0.96 MB |
| System ring (5s @ 48kHz stereo 16-bit) | ~0.96 MB |

Tokio runtime: `worker_threads=2`, `thread_stack_size=512 KiB`.

Audio frame dropping policy: if send queue is full, drop oldest frame and continue (prefer continuity over RAM growth).

## Installer size

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

## End-to-end testing checklist

1. **Build and run**: `bun run tauri dev`
2. **Start capture** (mock): Select "Mock (offline test)" provider, click Start. Verify partial/final transcript updates appear in the Live Transcript panel.
3. **Start capture** (real): Set API keys, select Deepgram or AssemblyAI, click Start. Play audio through a Google Meet or Teams call.
4. **Verify partial updates**: Transcript panel shows interim text updating in real time.
5. **Stop capture**: Click Stop. Verify:
   - Status shows "finalizing" then "stopped"
   - Meeting notes are generated (check the notes editor)
   - No errors in the console
6. **Export Markdown**: Click "Export MD" button. Verify a `.md` file is created in the app data directory.
7. **Summary recipe**: Click the "/Summary" chip. Verify it generates a summary from notes + transcript.
8. **No audio on disk**: Verify no `.wav`, `.pcm`, or audio files are created anywhere (only the encrypted `.db` and exported `.md`).
9. **Keys not leaked**: Open browser dev tools (if available) and verify no API keys appear in console logs or network events visible to the WebView.
10. **Settings view**: Click user profile card in sidebar. Verify Settings opens with API key rows. Save/delete a key and verify status updates.
11. **Notes view**: Navigate to My Notes. Verify standalone notes and meeting notes load. Create a note via the pencil icon, edit, and return.
12. **People view**: Navigate to People. Verify the data table renders. Add a person via the inline form.
13. **Companies view**: Navigate to Companies. Verify the data table renders. Add a company via the inline form.

## Architecture

```
index.html + src/main.js + src/style.css  (Tauri WebView UI)
        |
        | Tauri IPC (commands + events)
        |
src-tauri/src/
  lib.rs            — Tauri app setup
  main.rs           — entry point
  error.rs          — AudireError types
  state.rs          — AppState (store, keyvault, tokio runtime)
  ipc.rs            — Tauri commands (capture, notes, keys, participants, orgs)
  audio/
    mod.rs          — module declarations
    types.rs        — PcmFormat, AudioChunk
    ring.rs         — ringbuf ring buffer creation
    resample.rs     — mono mix, downsample 48k/44.1k->16k, frame 100ms
    mic_cpal.rs     — mic capture via cpal
    system_capture.rs — unified system capture dispatcher
    system_windows_wasapi.rs — WASAPI loopback (Windows)
    system_macos_sck.rs      — ScreenCaptureKit helper (macOS)
    system_linux_monitor.rs  — PulseAudio/PipeWire monitor (Linux)
  asr/
    mod.rs          — pipeline orchestration (capture + ASR + store)
    events.rs       — unified ASR event parsing + tests
    deepgram.rs     — Deepgram Flux v2 WebSocket client
    assemblyai.rs   — AssemblyAI U3 Pro WebSocket client
    mock.rs         — mock provider for offline testing
  store/
    mod.rs          — module export
    db.rs           — SQLCipher local store + FTS5 + participants/orgs/standalone_notes + tests
  keyvault/
    mod.rs          — module export
    vault.rs        — OS keyring + env var key vault
  llm/
    mod.rs          — module declarations
    recipe.rs       — post-transcription recipes (summary, etc.)
    openai.rs       — optional OpenAI gateway stub
    anthropic.rs    — optional Anthropic gateway stub
  bin/
    audire_keytool.rs — CLI for storing keys in OS keyring
```

## macOS helper

```
src-tauri/helpers/audire_sck_helper.swift
```

## Sources (primary references)

**Tauri v2:**
- Capabilities + command access model: https://v2.tauri.app/security/capabilities/
- Calling Rust from frontend: https://v2.tauri.app/develop/calling-rust/
- Windows installer + WebView2 modes/sizes: https://v2.tauri.app/distribute/windows-installer/

**ASR:**
- Deepgram Flux v2 Listen: https://developers.deepgram.com/reference/speech-to-text/listen-flux
- Deepgram Flux EOT tuning: https://developers.deepgram.com/docs/flux/configuration
- Deepgram Flux CloseStream: https://developers.deepgram.com/docs/flux/close-stream
- AssemblyAI Universal-3 Pro Streaming: https://www.assemblyai.com/docs/api-reference/streaming-api/universal-3-pro-streaming/universal-3-pro-streaming
- AssemblyAI ForceEndpoint guide: https://www.assemblyai.com/docs/streaming/universal-3-pro

**Audio:**
- Windows WASAPI loopback recording: https://learn.microsoft.com/en-us/windows/win32/coreaudio/loopback-recording
- Application loopback sample: https://learn.microsoft.com/en-us/samples/microsoft/windows-classic-samples/applicationloopbackaudio-sample/
- wasapi crate: https://docs.rs/wasapi/latest/wasapi/struct.AudioClient.html
- ScreenCaptureKit (WWDC22): https://developer.apple.com/videos/play/wwdc2022/10156/
- PulseAudio monitor sources: https://www.freedesktop.org/wiki/Software/PulseAudio/Documentation/User/Modules/
- PipeWire pw-cat: https://www.mankier.com/1/pw-cat

**Storage & Security:**
- SQLite FTS5: https://www.sqlite.org/fts5.html
- Tokio runtime Builder: https://docs.rs/tokio/latest/tokio/runtime/struct.Builder.html
