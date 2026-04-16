use crate::audio::ring;
use crate::audio::types::PcmFormat;
use crate::error::{ParaError, Result};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use ringbuf::{traits::Producer, HeapCons};

/// Wrapper to make cpal::Stream Send.
/// Safety: cpal::Stream is a handle to the platform audio system. The actual
/// audio callback runs on cpal's dedicated thread. Moving the handle between
/// threads is safe; we only use it for lifetime management (drop = stop capture).
#[allow(dead_code)]
struct SendStream(cpal::Stream);
// SAFETY: see above — the Stream is just a handle/guard.
unsafe impl Send for SendStream {}

/// Active mic capture. Holds the cpal Stream (keeps capture alive)
/// and the consumer end of the ring buffer for the pipeline to read.
pub struct MicCapture {
    pub fmt: PcmFormat,
    pub consumer: HeapCons<i16>,
    // Dropping _stream stops the cpal capture.
    _stream: SendStream,
}

/// Start capturing from the default input device (microphone).
///
/// Creates a ring buffer of `seconds_ring` seconds capacity.
/// The producer is moved into the cpal callback; the consumer is returned
/// for the ASR pipeline to read from.
///
/// Privacy: no audio data is written to disk.
pub fn start_mic_capture(seconds_ring: u32) -> Result<MicCapture> {
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .ok_or_else(|| ParaError::Audio("no default input device found".into()))?;

    let supported = device
        .default_input_config()
        .map_err(|e| ParaError::Audio(format!("input config: {}", e)))?;

    let sr = supported.sample_rate().0;
    let ch = supported.channels();
    let config = supported.config();

    // Ring buffer: seconds * sample_rate * channels (interleaved i16 samples).
    let capacity = (sr as usize) * (ch as usize) * (seconds_ring as usize);
    let (mut prod, cons) = ring::create_ring(capacity);

    let err_fn = |err: cpal::StreamError| {
        eprintln!("[audire] cpal mic stream error: {}", err);
    };

    let stream = match supported.sample_format() {
        cpal::SampleFormat::I16 => device.build_input_stream(
            &config,
            move |data: &[i16], _: &cpal::InputCallbackInfo| {
                for &s in data {
                    let _ = prod.try_push(s);
                }
            },
            err_fn,
            None,
        ),
        cpal::SampleFormat::F32 => {
            // Shared producer — we need a separate one for F32.
            // Actually we already moved prod above, so we recreate for this branch.
            // Rust: only one branch executes, so the move is fine.
            // But the borrow checker sees prod as moved in the I16 branch.
            // Fix: create the closure inside each branch with its own prod.
            // We'll restructure slightly.
            return start_mic_capture_f32(&device, &config, sr, ch, seconds_ring);
        }
        other => {
            return Err(ParaError::Audio(format!(
                "unsupported mic sample format: {:?}",
                other
            )));
        }
    }
    .map_err(|e| ParaError::Audio(format!("build stream: {}", e)))?;

    stream
        .play()
        .map_err(|e| ParaError::Audio(format!("play: {}", e)))?;

    Ok(MicCapture {
        fmt: PcmFormat {
            sample_rate: sr,
            channels: ch,
        },
        consumer: cons,
        _stream: SendStream(stream),
    })
}

/// Variant for F32 sample format (common on macOS/Linux).
fn start_mic_capture_f32(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    sr: u32,
    ch: u16,
    seconds_ring: u32,
) -> Result<MicCapture> {
    let capacity = (sr as usize) * (ch as usize) * (seconds_ring as usize);
    let (mut prod, cons) = ring::create_ring(capacity);

    let err_fn = |err: cpal::StreamError| {
        eprintln!("[audire] cpal mic stream error: {}", err);
    };

    let stream = device
        .build_input_stream(
            config,
            move |data: &[f32], _: &cpal::InputCallbackInfo| {
                for &s in data {
                    let v = (s * i16::MAX as f32).clamp(i16::MIN as f32, i16::MAX as f32) as i16;
                    let _ = prod.try_push(v);
                }
            },
            err_fn,
            None,
        )
        .map_err(|e| ParaError::Audio(format!("build stream f32: {}", e)))?;

    stream
        .play()
        .map_err(|e| ParaError::Audio(format!("play: {}", e)))?;

    Ok(MicCapture {
        fmt: PcmFormat {
            sample_rate: sr,
            channels: ch,
        },
        consumer: cons,
        _stream: SendStream(stream),
    })
}
