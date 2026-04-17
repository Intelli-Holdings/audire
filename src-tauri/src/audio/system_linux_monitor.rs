// Linux system audio capture via PulseAudio/PipeWire monitor sources.
//
// Strategy:
// 1. List cpal input devices on the default host.
// 2. Filter devices whose name contains "monitor" (PulseAudio convention:
//    sinks expose corresponding ".monitor" sources).
// 3. Open the first matching device with cpal, push i16 samples to ring buffer.
// 4. If no monitor found, return an explicit error with user guidance.
//
// Fallback: if PipeWire's pw-cat is installed and user enables it in settings,
// spawn `pw-cat --record --target=<node>` and read PCM from stdout.
// This is NOT the default — only used if explicitly enabled.
//
// References:
// - PulseAudio monitor sources: https://www.freedesktop.org/wiki/Software/PulseAudio/Documentation/User/Modules/
// - PipeWire pw-cat: https://www.mankier.com/1/pw-cat

use crate::audio::system_capture::SystemCapture;
use crate::error::{ParaError, Result};

#[cfg(target_os = "linux")]
use crate::audio::ring;
#[cfg(target_os = "linux")]
use crate::audio::types::PcmFormat;
#[cfg(target_os = "linux")]
use ringbuf::traits::Producer;
#[cfg(target_os = "linux")]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(target_os = "linux")]
use std::sync::Arc;

/// Start Linux system audio capture by finding a monitor source.
pub fn start_capture(seconds_ring: u32) -> Result<SystemCapture> {
    #[cfg(target_os = "linux")]
    {
        use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

        let host = cpal::default_host();

        // Find a monitor device (PulseAudio/PipeWire-pulse convention)
        let monitor_device = host
            .input_devices()
            .map_err(|e| ParaError::Audio(format!("enumerate inputs: {}", e)))?
            .find(|d| {
                d.name()
                    .map(|n| {
                        let lower = n.to_lowercase();
                        lower.contains("monitor") || lower.contains("loopback")
                    })
                    .unwrap_or(false)
            });

        let device = monitor_device.ok_or_else(|| {
            ParaError::Audio(
                "no PulseAudio/PipeWire monitor source found. \
                 To capture system audio on Linux:\n\
                 1. Ensure PulseAudio or PipeWire-pulse is running.\n\
                 2. The default sink should have a .monitor source.\n\
                 3. Run 'pactl list short sources' to verify.\n\
                 4. If using PipeWire without PulseAudio compat, you may need \
                    to configure a loopback device."
                    .into(),
            )
        })?;

        let dev_name = device.name().unwrap_or_else(|_| "unknown".to_string());
        eprintln!("[audire] Linux: using monitor source: {}", dev_name);

        let supported = device
            .default_input_config()
            .map_err(|e| ParaError::Audio(format!("monitor config: {}", e)))?;

        let sr = supported.sample_rate().0;
        let ch = supported.channels();
        let config = supported.config();

        // Ring buffer
        let capacity = (sr as usize) * (ch as usize) * (seconds_ring as usize);
        let (mut prod, cons) = ring::create_ring(capacity);

        let stop_flag = Arc::new(AtomicBool::new(false));
        let stop_flag_stream = stop_flag.clone();

        let err_fn = |err: cpal::StreamError| {
            eprintln!("[audire] linux monitor stream error: {}", err);
        };

        let stream = match supported.sample_format() {
            cpal::SampleFormat::I16 => device
                .build_input_stream(
                    &config,
                    move |data: &[i16], _: &cpal::InputCallbackInfo| {
                        if stop_flag_stream.load(Ordering::Relaxed) {
                            return;
                        }
                        for &s in data {
                            let _ = prod.try_push(s);
                        }
                    },
                    err_fn,
                    None,
                )
                .map_err(|e| ParaError::Audio(format!("build monitor stream: {}", e)))?,
            cpal::SampleFormat::F32 => {
                // F32 variant
                let capacity2 = (sr as usize) * (ch as usize) * (seconds_ring as usize);
                let (mut prod2, cons2) = ring::create_ring(capacity2);
                // We need to use cons2 instead of cons, but we already created cons above.
                // Restructure: return early with the F32 stream.
                let stop2 = stop_flag.clone();
                let stream = device
                    .build_input_stream(
                        &config,
                        move |data: &[f32], _: &cpal::InputCallbackInfo| {
                            if stop2.load(Ordering::Relaxed) {
                                return;
                            }
                            for &s in data {
                                let v = (s * i16::MAX as f32)
                                    .clamp(i16::MIN as f32, i16::MAX as f32)
                                    as i16;
                                let _ = prod2.try_push(v);
                            }
                        },
                        err_fn,
                        None,
                    )
                    .map_err(|e| ParaError::Audio(format!("build monitor stream f32: {}", e)))?;

                stream
                    .play()
                    .map_err(|e| ParaError::Audio(format!("play monitor: {}", e)))?;

                // Wrap in SendStream for thread safety (same pattern as mic_cpal)
                struct SendStream(cpal::Stream);
                unsafe impl Send for SendStream {}

                // We need to keep the stream alive. Store it in a thread that just parks.
                let send_stream = SendStream(stream);
                let stop_park = stop_flag.clone();
                let handle = std::thread::Builder::new()
                    .name("audire-linux-monitor-f32".into())
                    .spawn(move || {
                        let _s = send_stream; // keep alive
                        while !stop_park.load(Ordering::Relaxed) {
                            std::thread::sleep(std::time::Duration::from_millis(100));
                        }
                    })
                    .map_err(|e| ParaError::Audio(format!("spawn monitor thread: {}", e)))?;

                return Ok(SystemCapture {
                    fmt: PcmFormat {
                        sample_rate: sr,
                        channels: ch,
                    },
                    consumer: cons2,
                    _stop_flag: stop_flag,
                    _thread: Some(handle),
                });
            }
            other => {
                return Err(ParaError::Audio(format!(
                    "unsupported monitor sample format: {:?}",
                    other
                )));
            }
        };

        stream
            .play()
            .map_err(|e| ParaError::Audio(format!("play monitor: {}", e)))?;

        // Keep stream alive in a parking thread
        struct SendStream(cpal::Stream);
        unsafe impl Send for SendStream {}

        let send_stream = SendStream(stream);
        let stop_park = stop_flag.clone();
        let handle = std::thread::Builder::new()
            .name("audire-linux-monitor".into())
            .spawn(move || {
                let _s = send_stream;
                while !stop_park.load(Ordering::Relaxed) {
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
            })
            .map_err(|e| ParaError::Audio(format!("spawn monitor thread: {}", e)))?;

        Ok(SystemCapture {
            fmt: PcmFormat {
                sample_rate: sr,
                channels: ch,
            },
            consumer: cons,
            _stop_flag: stop_flag,
            _thread: Some(handle),
        })
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = seconds_ring;
        Err(ParaError::Audio(
            "Linux monitor capture is only available on Linux".into(),
        ))
    }
}
