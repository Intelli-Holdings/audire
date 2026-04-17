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
            Ok(m) => Some(m),
            Err(e) => {
                let _ = app.emit(
                    "asr:status",
                    serde_json::json!({ "status": format!("mic unavailable: {}", e) }),
                );
                None
            }
        }
    } else {
        None
    };

    // 2. Start system audio capture (platform-dependent)
    let sys_result = start_system_audio(&config);
    if let Err(ref e) = sys_result {
        let _ = app.emit(
            "asr:status",
            serde_json::json!({ "status": format!("system audio: {}", e) }),
        );
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
        loop {
            match events::recv_event(&mut ws_rx, &prov_rx).await {
                Ok(ev) => {
                    if ev.is_termination() {
                        break;
                    }
                    if let Some(p) = ev.partial_text() {
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
                }
                Err(_) => break,
            }
        }
    });

    // 5. Spawn sender task (audio ring -> resample -> WSS)
    let mut send_handle = spawn_audio_sender(mic, sys_result.ok(), ws_tx_send);

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
    let mut capture = state.capture.lock().unwrap();
    if capture.as_ref().map(|handle| handle.meeting_id.as_str()) == Some(meeting_id) {
        capture.take();
    }
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
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                continue;
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

            let frames = audio::resample::frame_16k_100ms(&mono_16k);
            for frame in frames {
                if ws_tx.send(Message::Binary(frame.into())).await.is_err() {
                    return;
                }
            }
        }
    })
}
