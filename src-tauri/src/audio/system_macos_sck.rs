// macOS system audio capture via ScreenCaptureKit.
//
// ScreenCaptureKit captures system audio output (what you hear) without requiring
// a virtual audio device. It requires the Screen Recording permission.
//
// Strategy: spawn a tiny Swift helper binary (included in the app bundle) that:
//   1. Requests Screen Recording permission via SCShareableContent.
//   2. Configures SCStream with capturesAudio=true, sampleRate=48000, channelCount=2.
//   3. Captures audio frames and writes raw i16 PCM to stdout.
//   4. Rust reads stdout and pushes samples into the ring buffer.
//
// This avoids needing Objective-C/Swift bindings in Rust while keeping the bundle small.
// The helper binary is ~200KB and does not write audio to disk.
//
// Reference: https://developer.apple.com/videos/play/wwdc2022/10156/
// Apple sample: https://github.com/Fidetro/CapturingScreenContentInMacOS

use crate::audio::system_capture::SystemCapture;
use crate::error::{ParaError, Result};

#[cfg(target_os = "macos")]
use crate::audio::ring;
#[cfg(target_os = "macos")]
use crate::audio::types::PcmFormat;
#[cfg(target_os = "macos")]
use ringbuf::traits::Producer;
#[cfg(target_os = "macos")]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(target_os = "macos")]
use std::sync::Arc;

/// Start macOS system audio capture via ScreenCaptureKit helper.
pub fn start_capture(seconds_ring: u32) -> Result<SystemCapture> {
    #[cfg(target_os = "macos")]
    {
        // Ring buffer: 48kHz * 2ch * N seconds
        let capacity = 48_000usize * 2 * seconds_ring as usize;
        let (mut prod, cons) = ring::create_ring(capacity);

        let stop_flag = Arc::new(AtomicBool::new(false));
        let stop_flag_thread = stop_flag.clone();

        // Locate the helper binary in the app bundle.
        // In dev: ./src-tauri/helpers/audire_sck_helper
        // In release bundle: ../MacOS/audire_sck_helper or ../Resources/
        let helper_path = find_sck_helper()?;

        let handle = std::thread::Builder::new()
            .name("audire-sck-capture".into())
            .spawn(move || {
                use std::io::Read;
                use std::process::{Command, Stdio};

                let mut child = match Command::new(&helper_path)
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
                {
                    Ok(c) => c,
                    Err(e) => {
                        eprintln!(
                            "[audire] SCK helper spawn failed: {}. \
                             Ensure Screen Recording permission is granted in \
                             System Settings > Privacy & Security > Screen Recording.",
                            e
                        );
                        return;
                    }
                };

                let mut stdout = match child.stdout.take() {
                    Some(s) => s,
                    None => return,
                };

                // Read raw i16 PCM from helper stdout (interleaved stereo, 48kHz).
                // Each sample is 2 bytes (little-endian i16).
                let mut buf = [0u8; 4096];
                while !stop_flag_thread.load(Ordering::Relaxed) {
                    match stdout.read(&mut buf) {
                        Ok(0) => break, // helper exited
                        Ok(n) => {
                            // Convert pairs of bytes to i16 samples
                            let pairs = n / 2;
                            for i in 0..pairs {
                                let lo = buf[i * 2] as i16;
                                let hi = (buf[i * 2 + 1] as i16) << 8;
                                let sample = lo | hi;
                                let _ = prod.try_push(sample);
                            }
                        }
                        Err(e) => {
                            eprintln!("[audire] SCK helper read error: {}", e);
                            break;
                        }
                    }
                }

                // Kill helper when we're done
                let _ = child.kill();
                let _ = child.wait();

                // Check stderr for permission errors
                if let Some(mut stderr) = child.stderr.take() {
                    let mut err_msg = String::new();
                    let _ = stderr.read_to_string(&mut err_msg);
                    if !err_msg.is_empty() {
                        eprintln!("[audire] SCK helper stderr: {}", err_msg);
                    }
                }
            })
            .map_err(|e| ParaError::Audio(format!("spawn sck thread: {}", e)))?;

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

    #[cfg(not(target_os = "macos"))]
    {
        let _ = seconds_ring;
        Err(ParaError::Audio(
            "macOS ScreenCaptureKit capture is only available on macOS".into(),
        ))
    }
}

/// Locate the SCK helper binary.
#[cfg(target_os = "macos")]
fn find_sck_helper() -> Result<std::path::PathBuf> {
    // Check common locations:
    // 1. Next to the main binary (release bundle)
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()));

    if let Some(ref dir) = exe_dir {
        let candidate = dir.join("audire_sck_helper");
        if candidate.exists() {
            return Ok(candidate);
        }
    }

    // 2. In Resources directory (Tauri macOS bundle)
    if let Some(ref dir) = exe_dir {
        let candidate = dir
            .parent()
            .map(|d| d.join("Resources").join("audire_sck_helper"));
        if let Some(c) = candidate {
            if c.exists() {
                return Ok(c);
            }
        }
    }

    // 3. Dev mode: helpers directory
    let dev_candidate = std::path::PathBuf::from("helpers/audire_sck_helper");
    if dev_candidate.exists() {
        return Ok(dev_candidate);
    }

    Err(ParaError::Audio(
        "ScreenCaptureKit helper binary not found. \
         macOS system audio capture requires the audire_sck_helper binary. \
         See README for build instructions. \
         Grant Screen Recording permission in System Settings > Privacy & Security."
            .into(),
    ))
}
