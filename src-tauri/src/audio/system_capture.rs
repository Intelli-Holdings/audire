// Unified system audio capture abstraction.
//
// Dispatches to the appropriate platform backend:
// - Windows: WASAPI loopback (system_windows_wasapi.rs)
// - macOS: ScreenCaptureKit (system_macos_sck.rs)
// - Linux: PulseAudio/PipeWire monitor source (system_linux_monitor.rs)
//
// All backends share the same SystemCapture output type.

use crate::audio::types::PcmFormat;
#[allow(unused_imports)]
use crate::error::{ParaError, Result};
use ringbuf::HeapCons;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Active system audio capture handle.
/// Dropping this struct stops the capture.
pub struct SystemCapture {
    pub fmt: PcmFormat,
    pub consumer: HeapCons<i16>,
    /// Shared stop flag — set to true to signal the capture thread to exit.
    pub(crate) _stop_flag: Arc<AtomicBool>,
    /// Thread handle (if platform uses a dedicated thread).
    pub(crate) _thread: Option<std::thread::JoinHandle<()>>,
}

impl SystemCapture {
    /// Create a new SystemCapture.
    pub fn new(
        fmt: PcmFormat,
        consumer: HeapCons<i16>,
        stop_flag: Arc<AtomicBool>,
        thread: Option<std::thread::JoinHandle<()>>,
    ) -> Self {
        Self {
            fmt,
            consumer,
            _stop_flag: stop_flag,
            _thread: thread,
        }
    }
}

impl Drop for SystemCapture {
    fn drop(&mut self) {
        self._stop_flag.store(true, Ordering::Relaxed);
    }
}

/// Start system audio capture.
///
/// # Arguments
/// * `seconds_ring` — ring buffer capacity in seconds.
/// * `mode` — "system" for default loopback, "process" for per-app capture (Windows only).
/// * `target_pid` — optional process ID for per-app capture.
///
/// Privacy: no audio data is written to disk.
pub fn start_system_audio_capture(
    seconds_ring: u32,
    mode: &str,
    target_pid: Option<u32>,
) -> Result<SystemCapture> {
    #[cfg(target_os = "windows")]
    {
        super::system_windows_wasapi::start_capture(seconds_ring, mode, target_pid)
    }

    #[cfg(target_os = "macos")]
    {
        let _ = (mode, target_pid);
        super::system_macos_sck::start_capture(seconds_ring)
    }

    #[cfg(target_os = "linux")]
    {
        let _ = (mode, target_pid);
        super::system_linux_monitor::start_capture(seconds_ring)
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        let _ = (seconds_ring, mode, target_pid);
        Err(ParaError::Audio(
            "system audio capture not supported on this platform".into(),
        ))
    }
}
