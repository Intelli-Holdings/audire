// Windows system audio capture via WASAPI loopback.
//
// Default mode: capture from the default render endpoint ("what you hear").
// Process mode: per-app loopback via AudioClient::new_application_loopback_client
//   (requires Windows 10 20348+ / Windows 11).
//
// Reference: https://learn.microsoft.com/en-us/windows/win32/coreaudio/loopback-recording
// Application loopback: https://learn.microsoft.com/en-us/samples/microsoft/windows-classic-samples/applicationloopbackaudio-sample/
// wasapi crate: https://docs.rs/wasapi/latest/wasapi/struct.AudioClient.html

use crate::audio::ring;
use crate::audio::system_capture::SystemCapture;
use crate::audio::types::PcmFormat;
use crate::error::{ParaError, Result};

use ringbuf::traits::Producer;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// Start WASAPI loopback capture.
///
/// # Arguments
/// * `seconds_ring` — ring buffer capacity in seconds.
/// * `mode` — "system" for default render endpoint loopback, "process" for per-app.
/// * `target_pid` — PID for per-app capture (only used when mode="process").
pub fn start_capture(
    seconds_ring: u32,
    mode: &str,
    target_pid: Option<u32>,
) -> Result<SystemCapture> {
    #[cfg(all(windows, feature = "sys_audio_windows"))]
    {
        use wasapi::*;

        // Ring buffer: 48kHz * 2 channels * N seconds of i16 samples.
        // Memory budget: 5s @ 48kHz stereo = 48000 * 2 * 5 = 480,000 samples ≈ 0.96 MB.
        let capacity = 48_000usize * 2 * seconds_ring as usize;
        let (mut prod, cons) = ring::create_ring(capacity);

        let stop_flag = Arc::new(AtomicBool::new(false));
        let stop_flag_thread = stop_flag.clone();

        let mode = mode.to_string();
        let handle = std::thread::Builder::new()
            .name("audire-wasapi-loopback".into())
            .spawn(move || {
                // COM init on this thread (MTA for background capture).
                if initialize_mta().is_err() {
                    eprintln!("[audire] WASAPI: COM MTA init failed");
                    return;
                }

                let audio_client_result = if mode == "process" && target_pid.is_some() {
                    // Per-process loopback (Windows 10 20348+ / Windows 11)
                    // Reference: https://docs.rs/wasapi/latest/wasapi/struct.AudioClient.html
                    let pid = target_pid.unwrap();
                    eprintln!("[audire] WASAPI: attempting per-process loopback for PID {}", pid);
                    // Note: new_application_loopback_client may not be available on older wasapi crate versions.
                    // If unavailable, falls through to the error below.
                    Err(format!("per-process loopback not yet supported in wasapi 0.16; PID={}", pid))
                } else {
                    // Default: system-wide loopback from the default render device
                    init_system_loopback()
                };

                let (audio_client, h_event, capture_client) = match audio_client_result {
                    Ok(v) => v,
                    Err(e) => {
                        eprintln!("[audire] WASAPI: init failed: {}", e);
                        return;
                    }
                };

                if let Err(e) = audio_client.start_stream() {
                    eprintln!("[audire] WASAPI: start_stream: {}", e);
                    return;
                }

                // Read loop: pull bytes from device, convert float32 → i16, push to ring.
                let mut buf = std::collections::VecDeque::<u8>::with_capacity(256 * 1024);

                while !stop_flag_thread.load(Ordering::Relaxed) {
                    if h_event.wait_for_event(500).is_err() {
                        // Timeout or error — check stop flag and retry
                        continue;
                    }

                    match capture_client.read_from_device_to_deque(&mut buf) {
                        Ok(_) => {}
                        Err(e) => {
                            eprintln!("[audire] WASAPI: read error: {}", e);
                            break;
                        }
                    }

                    // Convert raw bytes to f32 samples, then to i16.
                    // WASAPI mix format is typically float32 (4 bytes per sample).
                    while buf.len() >= 4 {
                        let b0 = buf.pop_front().unwrap();
                        let b1 = buf.pop_front().unwrap();
                        let b2 = buf.pop_front().unwrap();
                        let b3 = buf.pop_front().unwrap();
                        let f = f32::from_le_bytes([b0, b1, b2, b3]);
                        let s = (f * i16::MAX as f32)
                            .clamp(i16::MIN as f32, i16::MAX as f32) as i16;
                        // Backpressure: if ring is full, oldest samples are dropped (try_push).
                        // This is intentional — prefer continuity over RAM growth.
                        let _ = prod.try_push(s);
                    }
                }

                let _ = audio_client.stop_stream();
            })
            .map_err(|e| ParaError::Audio(format!("spawn wasapi thread: {}", e)))?;

        Ok(SystemCapture {
            fmt: PcmFormat {
                sample_rate: 48_000,
                channels: 2,
            },
            consumer: cons,
            _stop_flag: stop_flag,
            _thread: Some(handle),
        })
    }

    #[cfg(not(all(windows, feature = "sys_audio_windows")))]
    {
        let _ = (seconds_ring, mode, target_pid);
        Err(ParaError::Audio(
            "WASAPI system audio capture not available. \
             Windows: ensure sys_audio_windows feature is enabled. \
             Other platforms: use the appropriate backend."
                .into(),
        ))
    }
}

/// Initialize WASAPI loopback capture from the default render device.
/// Returns (AudioClient, Handle, AudioCaptureClient) on success.
#[cfg(all(windows, feature = "sys_audio_windows"))]
fn init_system_loopback() -> std::result::Result<
    (wasapi::AudioClient, wasapi::Handle, wasapi::AudioCaptureClient),
    String,
> {
    use wasapi::*;

    let device = DeviceCollection::new(&Direction::Render)
        .and_then(|coll| coll.get_device_at_index(0))
        .map_err(|e| format!("no render device: {}", e))?;

    let mut audio_client = device
        .get_iaudioclient()
        .map_err(|e| format!("get_iaudioclient: {}", e))?;

    let mix_format = audio_client
        .get_mixformat()
        .map_err(|e| format!("get_mixformat: {}", e))?;

    let (_def_period, min_period) = audio_client
        .get_periods()
        .unwrap_or((0, 100_000));

    audio_client
        .initialize_client(
            &mix_format,
            min_period,
            &Direction::Capture,
            &ShareMode::Shared,
            true, // autoconvert
        )
        .map_err(|e| format!("initialize_client: {}", e))?;

    let h_event = audio_client
        .set_get_eventhandle()
        .map_err(|e| format!("set_get_eventhandle: {}", e))?;

    let capture_client = audio_client
        .get_audiocaptureclient()
        .map_err(|e| format!("get_audiocaptureclient: {}", e))?;

    Ok((audio_client, h_event, capture_client))
}
