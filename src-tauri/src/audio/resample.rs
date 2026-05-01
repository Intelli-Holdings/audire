use crate::error::{ParaError, Result};

/// Mix interleaved stereo (or N-channel) i16 samples to mono by averaging.
/// If input is already mono, returns it unchanged.
pub fn to_mono_i16(interleaved: &[i16], channels: u16) -> Vec<i16> {
    if channels <= 1 {
        return interleaved.to_vec();
    }
    let ch = channels as usize;
    let frames = interleaved.len() / ch;
    let mut out = Vec::with_capacity(frames);
    for f in 0..frames {
        let mut acc: i32 = 0;
        for c in 0..ch {
            acc += interleaved[f * ch + c] as i32;
        }
        out.push((acc / ch as i32) as i16);
    }
    out
}

/// Mix two mono audio sources by averaging. If one is empty, returns the other.
/// Handles different lengths by mixing up to the shorter and appending the remainder.
pub fn mix_sources(a: &[i16], b: &[i16]) -> Vec<i16> {
    if a.is_empty() {
        return b.to_vec();
    }
    if b.is_empty() {
        return a.to_vec();
    }
    let min_len = a.len().min(b.len());
    let max_len = a.len().max(b.len());
    let mut out = Vec::with_capacity(max_len);
    for i in 0..min_len {
        let v = (a[i] as i32 + b[i] as i32) / 2;
        out.push(v.clamp(i16::MIN as i32, i16::MAX as i32) as i16);
    }
    // Append remainder from whichever is longer (halved to maintain level)
    let longer = if a.len() > b.len() { a } else { b };
    for i in min_len..max_len {
        out.push(longer[i] / 2);
    }
    out
}

/// Downsample mono audio to 16 kHz.
///
/// - If input_sr == 48000: simple decimation by 3 (take every 3rd sample).
///   Speech is already band-limited by mic/meeting codecs, so this is adequate for ASR.
/// - If input_sr == 16000: passthrough (already at target rate).
/// - If input_sr == 44100: nearest-sample resampling (acceptable for speech ASR).
/// - Other rates: returns an error.
pub fn downsample_to_16k_mono(input_mono: &[i16], input_sr: u32) -> Result<Vec<i16>> {
    match input_sr {
        16_000 => Ok(input_mono.to_vec()),
        48_000 => {
            // Decimate by 3: 48000/3 = 16000
            let mut out = Vec::with_capacity(input_mono.len() / 3 + 1);
            for i in (0..input_mono.len()).step_by(3) {
                out.push(input_mono[i]);
            }
            Ok(out)
        }
        44_100 => {
            // Nearest-sample resampling: 44100 -> 16000 (ratio ≈ 0.3628)
            let ratio = 16_000.0 / 44_100.0;
            let out_len = (input_mono.len() as f64 * ratio) as usize;
            let mut out = Vec::with_capacity(out_len);
            for i in 0..out_len {
                let src_idx = (i as f64 / ratio) as usize;
                if src_idx < input_mono.len() {
                    out.push(input_mono[src_idx]);
                }
            }
            Ok(out)
        }
        _ => Err(ParaError::Audio(format!(
            "unsupported input sample_rate={} for downsampler; expected 48000, 44100, or 16000.",
            input_sr
        ))),
    }
}

/// Build ~100 ms frames of 16 kHz mono i16 PCM.
/// 16000 Hz * 0.1 s = 1600 samples per frame -> 3200 bytes (pcm_s16le).
/// Meets AssemblyAI 50-1000 ms chunk guidance and works with Deepgram Flux.
pub fn frame_16k_100ms(samples_16k: &[i16]) -> Vec<Vec<u8>> {
    const FRAME_SAMPLES: usize = 1600; // 100ms @ 16kHz
    let mut frames = Vec::new();
    let mut i = 0;
    while i + FRAME_SAMPLES <= samples_16k.len() {
        let slice = &samples_16k[i..i + FRAME_SAMPLES];
        let mut bytes = Vec::with_capacity(FRAME_SAMPLES * 2);
        for s in slice {
            bytes.extend_from_slice(&s.to_le_bytes());
        }
        frames.push(bytes);
        i += FRAME_SAMPLES;
    }
    frames
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_to_mono_stereo() {
        let stereo = vec![100i16, 200, 300, 400, 500, 600];
        let mono = to_mono_i16(&stereo, 2);
        assert_eq!(mono, vec![150, 350, 550]);
    }

    #[test]
    fn test_to_mono_passthrough() {
        let mono = vec![100i16, 200, 300];
        assert_eq!(to_mono_i16(&mono, 1), mono);
    }

    #[test]
    fn test_mix_sources_both() {
        let a = vec![1000i16, 2000, 3000];
        let b = vec![500i16, 1000, 1500];
        let mixed = mix_sources(&a, &b);
        assert_eq!(mixed, vec![750, 1500, 2250]);
    }

    #[test]
    fn test_mix_sources_one_empty() {
        let a = vec![1000i16, 2000];
        assert_eq!(mix_sources(&a, &[]), a);
        assert_eq!(mix_sources(&[], &a), a);
    }

    #[test]
    fn test_downsample_48k() {
        let input: Vec<i16> = (0..4800).map(|i| (i % 1000) as i16).collect();
        let output = downsample_to_16k_mono(&input, 48_000).unwrap();
        assert_eq!(output.len(), 1600);
        // First sample should be 0, fourth should be 9 (index 9 / step 3)
        assert_eq!(output[0], 0);
        assert_eq!(output[1], 3);
    }

    #[test]
    fn test_downsample_16k_passthrough() {
        let input = vec![1i16, 2, 3, 4, 5];
        let output = downsample_to_16k_mono(&input, 16_000).unwrap();
        assert_eq!(output, input);
    }

    #[test]
    fn test_frame_100ms() {
        let samples: Vec<i16> = vec![42; 3200]; // 200ms worth
        let frames = frame_16k_100ms(&samples);
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0].len(), 3200); // 1600 samples * 2 bytes
    }
}
