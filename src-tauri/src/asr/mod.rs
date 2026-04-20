pub mod assemblyai;
pub mod deepgram;
pub mod events;
pub mod mock;

use crate::audio;
use crate::error::{ParaError, Result};
use crate::services::meeting_notes;
use crate::state::AppState;
use crate::store::db::LocalStore;

use ringbuf::traits::Consumer;
use tauri::{Emitter, Manager};
use tokio::sync::oneshot;
use tokio_tungstenite::tungstenite::Message;

/// Capture mode for the pipeline.
#[derive(Clone, Debug)]
pub struct CaptureConfig {
    pub provider: String,
    pub include_mic: bool,
    /// "system" (default loopback) or "process" (per-app loopback on Windows 20348+)
    pub mode: String,
    /// Optional target process PID for per-process capture.
    pub target_process: Option<u32>,
}

/// Run the full capture -> ASR pipeline.
///
/// - Starts mic capture if include_mic is true (cross-platform via cpal).
/// - Starts system audio capture (WASAPI loopback on Windows, ScreenCaptureKit on macOS,
///   PulseAudio/PipeWire monitor on Linux).
/// - Connects to the chosen ASR provider's WebSocket.
/// - Pumps audio frames (16k mono pcm_s16le ~100ms) to ASR.
/// - Receives transcripts, emits events to UI, stores finals in DB.
/// - Runs until `stop_rx` fires, then sends finalize/terminate and drains.
///
/// Privacy: no audio is written to disk at any point.
pub async fn run_pipeline(
    app: tauri::AppHandle,
    store: LocalStore,
    meeting_id: String,
    config: CaptureConfig,
    api_key: String,
    stop_rx: oneshot::Receiver<()>,
) -> Result<()> {
    // 1. Start mic capture (optional)
    let mic = if config.include_mic {
        match audio::mic_cpal::start_mic_capture(5) {
            Ok(m) => {
                eprintln!("[asr] mic capture started: rate={} ch={}", m.fmt.sample_rate, m.fmt.channels);
                Some(m)
            }
            Err(e) => {
                eprintln!("[asr] mic capture failed: {}", e);
                let _ = app.emit(
                    "asr:status",
                    serde_json::json!({ "status": format!("mic unavailable: {}", e) }),
                );
                None
            }
        }
    } else {
        eprintln!("[asr] mic capture skipped (include_mic=false)");
        None
    };

    // 2. Start system audio capture (platform-dependent)
    let sys_result = start_system_audio(&config);
    match &sys_result {
        Ok(s) => eprintln!("[asr] system audio started: rate={} ch={}", s.fmt.sample_rate, s.fmt.channels),
        Err(ref e) => {
            eprintln!("[asr] system audio failed: {}", e);
            let _ = app.emit(
                "asr:status",
                serde_json::json!({ "status": format!("system audio: {}", e) }),
            );
        }
    }

    if mic.is_none() && sys_result.is_err() {
        return Err(ParaError::Audio(
            "no audio source available: both mic and system audio failed".into(),
        ));
    }

    let _ = app.emit(
        "asr:status",
        serde_json::json!({ "status": "capturing audio" }),
    );

    // 3. Connect to ASR WebSocket (or start mock)
    let (ws_tx, ws_rx) = match config.provider.as_str() {
        "deepgram" => deepgram::connect(&api_key).await?,
        "assemblyai" => assemblyai::connect(&api_key).await?,
        "mock" => mock::start_mock().await,
        _ => {
            return Err(ParaError::Asr(format!(
                "unknown provider: {}",
                config.provider
            )))
        }
    };

    let _ = app.emit(
        "asr:status",
        serde_json::json!({ "status": format!("streaming to {}", config.provider) }),
    );
    let _ = app.emit(
        "asr:lifecycle",
        serde_json::json!({
            "state": "running",
            "meeting_id": meeting_id.clone(),
            "provider": config.provider.clone(),
        }),
    );

    let ws_tx_send = ws_tx.clone();
    let ws_tx_stop = ws_tx;

    // 4. Spawn receiver task (ASR -> UI + DB)
    let app_rx = app.clone();
    let store_rx = store.clone();
    let meeting_rx = meeting_id.clone();
    let prov_rx = config.provider.clone();
    let mut ws_rx = ws_rx;

    let mut recv_handle = tokio::spawn(async move {
        let mut msg_count: u64 = 0;
        let mut first_partial_logged = false;
        let mut first_final_logged = false;
        loop {
            match events::recv_event(&mut ws_rx, &prov_rx).await {
                Ok(ev) => {
                    msg_count += 1;
                    if msg_count <= 5 {
                        eprintln!("[asr] msg #{} from {} (type: {})", msg_count, prov_rx, ev.event_type());
                    }
                    if ev.is_termination() {
                        eprintln!("[asr] received termination event from {}", prov_rx);
                        break;
                    }

                    let has_partial = ev.partial_text().is_some();
                    let has_final = ev.final_text().is_some();

                    if let Some(p) = ev.partial_text() {
                        if !first_partial_logged {
                            eprintln!("[asr] first partial received: \"{}\"", &p[..p.len().min(80)]);
                            first_partial_logged = true;
                        }
                        let formatted = ev.is_formatted();
                        let _ = app_rx.emit(
                            "asr:partial",
                            serde_json::json!({
                                "provider": prov_rx,
                                "text": p,
                                "formatted": formatted,
                            }),
                        );
                    }
                    if let Some(f) = ev.final_text() {
                        if !first_final_logged {
                            eprintln!("[asr] first final received: \"{}\"", &f[..f.len().min(80)]);
                            first_final_logged = true;
                        }
                        let ts_ms = chrono::Utc::now().timestamp_millis();
                        let formatted = ev.is_formatted();
                        let _ = app_rx.emit(
                            "asr:final",
                            serde_json::json!({
                                "provider": prov_rx,
                                "text": f,
                                "ts_ms": ts_ms,
                                "formatted": formatted,
                            }),
                        );
                        let _ = store_rx.insert_segment(&meeting_rx, "SYS", ts_ms, &f, None);
                    }

                    // Log messages that don't match any known pattern (Begin, Error, etc.)
                    if !has_partial && !has_final && !ev.is_termination() {
                        if let Some(raw) = ev.raw_json() {
                            let msg_type = raw.get("type").and_then(|t| t.as_str()).unwrap_or("unknown");
                            eprintln!("[asr] unhandled msg type '{}' from {}: {}", msg_type, prov_rx,
                                serde_json::to_string(raw).unwrap_or_default().chars().take(300).collect::<String>());
                        }
                    }
                }
                Err(e) => {
                    eprintln!("[asr] recv_event error (after {} msgs): {}", msg_count, e);
                    break;
                }
            }
        }
        eprintln!("[asr] receiver loop exited after {} messages", msg_count);
    });

    // 5. Spawn sender task (audio ring -> resample -> WSS)
    let mut send_handle = spawn_audio_sender(app.clone(), mic, sys_result.ok(), ws_tx_send);

    // 6. Wait for stop signal
    tokio::select! {
        _ = stop_rx => {
            let _ = app.emit(
                "asr:status",
                serde_json::json!({ "status": "finalizing" }),
            );

            match config.provider.as_str() {
                "deepgram" => {
                    deepgram::send_close_stream(&ws_tx_stop).await;
                }
                "assemblyai" => {
                    assemblyai::send_terminate_sequence(&ws_tx_stop).await;
                }
                _ => {}
            }

            let _ = tokio::time::timeout(
                std::time::Duration::from_secs(5),
                recv_handle,
            ).await;

            send_handle.abort();

            let _ = meeting_notes::generate_and_store(&store, &meeting_id, None);

            let _ = app.emit(
                "asr:status",
                serde_json::json!({ "status": "stopped" }),
            );
            let _ = app.emit(
                "asr:lifecycle",
                serde_json::json!({
                    "state": "stopped",
                    "meeting_id": meeting_id.clone(),
                }),
            );
        }
        _ = &mut recv_handle => {
            send_handle.abort();
            clear_capture_if_active(&app, &meeting_id);
            return Err(ParaError::Asr(
                "transcription stream ended before capture was stopped".into(),
            ));
        }
        _ = &mut send_handle => {
            recv_handle.abort();
            clear_capture_if_active(&app, &meeting_id);
            return Err(ParaError::Audio(
                "audio capture stream stopped unexpectedly".into(),
            ));
        }
    }

    Ok(())
}

fn clear_capture_if_active(app: &tauri::AppHandle, meeting_id: &str) {
    let state = app.state::<AppState>();
    if let Ok(mut capture) = state.capture.lock() {
        if capture.as_ref().map(|handle| handle.meeting_id.as_str()) == Some(meeting_id) {
            capture.take();
        }
    };
}

/// Start system audio capture based on platform and config.
fn start_system_audio(config: &CaptureConfig) -> Result<audio::system_capture::SystemCapture> {
    audio::system_capture::start_system_audio_capture(
        5,
        config.mode.as_str(),
        config.target_process,
    )
}

/// Spawn a task that drains audio ring buffers, resamples, and sends to WSS.
fn spawn_audio_sender(
    app: tauri::AppHandle,
    mic: Option<audio::mic_cpal::MicCapture>,
    sys: Option<audio::system_capture::SystemCapture>,
    ws_tx: tokio::sync::mpsc::Sender<Message>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut mic = mic;
        let mut sys = sys;
        let mut scratch_mic = Vec::<i16>::with_capacity(48_000);
        let mut scratch_sys = Vec::<i16>::with_capacity(48_000);
        let ws_tx = ws_tx;
        let mut first_frame_logged = false;

        // Accumulation buffer: downsampled 16kHz samples persist across loop
        // iterations so partial chunks (<1600 samples) are not discarded.
        let mut pending_16k = Vec::<i16>::with_capacity(4800);

        // Empty-state tracking: log only on transitions to avoid spam.
        let mut was_empty = false;
        let mut empty_since: Option<std::time::Instant> = None;

        // Pre-built 100ms of silence at 16kHz mono pcm_s16le = 1600 samples × 2 bytes
        // (3200 bytes). Sent as keep-alive when both buffers are empty so
        // AssemblyAI/Deepgram do not terminate the session on apparent inactivity
        // (e.g. WASAPI loopback emits nothing while the system is silent).
        let silence_frame: Vec<u8> = vec![0u8; 1600 * 2];
        let mut last_send_at = std::time::Instant::now();
        // Emit silence every ~200ms of real-time silence.
        let keepalive_interval = std::time::Duration::from_millis(200);

        let mut total_frames_sent: u64 = 0;

        loop {
            scratch_mic.clear();
            scratch_sys.clear();

            if let Some(ref mut m) = mic {
                let fmt = m.fmt;
                let drain_count = (fmt.sample_rate as usize * fmt.channels as usize) / 10;
                for _ in 0..drain_count {
                    match m.consumer.try_pop() {
                        Some(s) => scratch_mic.push(s),
                        None => break,
                    }
                }
            }

            if let Some(ref mut s) = sys {
                let fmt = s.fmt;
                let drain_count = (fmt.sample_rate as usize * fmt.channels as usize) / 10;
                for _ in 0..drain_count {
                    match s.consumer.try_pop() {
                        Some(sample) => scratch_sys.push(sample),
                        None => break,
                    }
                }
            }

            if scratch_mic.is_empty() && scratch_sys.is_empty() {
                // Log only on transition to empty (not every iteration).
                if !was_empty {
                    eprintln!("[asr] audio sender: both buffers empty, sending silence keep-alive");
                    was_empty = true;
                    empty_since = Some(std::time::Instant::now());
                }

                // Silence keep-alive: prevents providers from terminating a session
                // when the user is not speaking and nothing is playing through the
                // system loopback.
                if last_send_at.elapsed() >= keepalive_interval {
                    if ws_tx
                        .send(Message::Binary(silence_frame.clone().into()))
                        .await
                        .is_err()
                    {
                        eprintln!("[asr] audio sender: ws_tx send failed during keep-alive, exiting");
                        return;
                    }
                    let _ = app.emit(
                        "asr:audio_level",
                        serde_json::json!({ "level": 0.0f32 }),
                    );
                    last_send_at = std::time::Instant::now();
                }

                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                continue;
            }

            // Log transition from empty → data (with duration summary).
            if was_empty {
                let gap_ms = empty_since
                    .map(|t| t.elapsed().as_millis())
                    .unwrap_or(0);
                eprintln!("[asr] audio resumed after {}ms empty", gap_ms);
                was_empty = false;
                empty_since = None;
            }

            let mic_fmt = mic.as_ref().map(|m| m.fmt);
            let sys_fmt = sys.as_ref().map(|s| s.fmt);

            let mono_mic = if !scratch_mic.is_empty() {
                let ch = mic_fmt.map(|f| f.channels).unwrap_or(1);
                audio::resample::to_mono_i16(&scratch_mic, ch)
            } else {
                Vec::new()
            };

            let mono_sys = if !scratch_sys.is_empty() {
                let ch = sys_fmt.map(|f| f.channels).unwrap_or(2);
                audio::resample::to_mono_i16(&scratch_sys, ch)
            } else {
                Vec::new()
            };

            let mono_48k = audio::resample::mix_sources(&mono_mic, &mono_sys);

            if mono_48k.is_empty() {
                continue;
            }

            // Compute audio level from pre-downsampled data.
            let level = compute_audio_level(&mono_48k);
            let _ = app.emit(
                "asr:audio_level",
                serde_json::json!({ "level": level }),
            );

            let input_sr = sys_fmt
                .map(|f| f.sample_rate)
                .or_else(|| mic_fmt.map(|f| f.sample_rate))
                .unwrap_or(48_000);

            let mono_16k = match audio::resample::downsample_to_16k_mono(&mono_48k, input_sr) {
                Ok(v) => v,
                Err(_) => {
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                    continue;
                }
            };

            // Append to persistent accumulation buffer (fixes the core bug:
            // each iteration yields ~160 samples at 16kHz, but a frame needs
            // 1600. Without accumulation, remainders were silently discarded).
            pending_16k.extend_from_slice(&mono_16k);

            // Consume complete 1600-sample frames from the accumulation buffer.
            const FRAME_SAMPLES: usize = 1600; // 100ms @ 16kHz
            while pending_16k.len() >= FRAME_SAMPLES {
                let frame_samples: Vec<i16> = pending_16k.drain(..FRAME_SAMPLES).collect();
                let mut frame_bytes = Vec::with_capacity(FRAME_SAMPLES * 2);
                for s in &frame_samples {
                    frame_bytes.extend_from_slice(&s.to_le_bytes());
                }

                if !first_frame_logged {
                    eprintln!("[asr] first audio frame sent to WSS: {} bytes (pending_16k had {} samples)",
                        frame_bytes.len(), frame_samples.len() + pending_16k.len());
                    first_frame_logged = true;
                }
                total_frames_sent += 1;
                if total_frames_sent % 100 == 0 {
                    eprintln!("[asr] audio sender: {} frames sent (10s of audio)", total_frames_sent);
                }
                if ws_tx.send(Message::Binary(frame_bytes.into())).await.is_err() {
                    eprintln!("[asr] audio sender: ws_tx send failed, exiting");
                    return;
                }
                last_send_at = std::time::Instant::now();
            }
            // Unconsumed remainder stays in pending_16k for next iteration.
        }
    })
}

fn compute_audio_level(samples: &[i16]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }

    let sum_squares = samples.iter().fold(0.0f64, |acc, sample| {
        let normalized = *sample as f64 / i16::MAX as f64;
        acc + normalized * normalized
    });
    let rms = (sum_squares / samples.len() as f64).sqrt() as f32;
    (rms * 5.5).clamp(0.0, 1.0)
}
