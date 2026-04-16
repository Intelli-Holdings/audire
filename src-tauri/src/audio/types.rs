/// PCM format descriptor for an audio stream.
#[derive(Clone, Copy, Debug)]
pub struct PcmFormat {
    pub sample_rate: u32,
    pub channels: u16,
}

/// A chunk of interleaved i16 PCM audio data.
#[derive(Clone, Debug)]
pub struct AudioChunk {
    pub fmt: PcmFormat,
    pub data_i16: Vec<i16>,
}
