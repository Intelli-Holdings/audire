use ringbuf::{HeapCons, HeapProd, HeapRb};
use ringbuf::traits::Split; 
/// Create a bounded ring buffer for audio samples.
/// Returns (producer, consumer) — ownership is split.
///
/// Privacy: ring buffers live in memory only. No audio written to disk.
///
/// # Arguments
/// * `capacity_samples` — max samples before oldest are overwritten.
///   Example: 5s @48kHz stereo = 48000 * 2 * 5 = 480,000 samples ≈ 0.96MB.

pub fn create_ring(capacity_samples: usize) -> (HeapProd<i16>, HeapCons<i16>) {
    let rb = HeapRb::<i16>::new(capacity_samples);
    rb.split()
}