//! Matches a retained PCM tail against a replacement stream without modifying encoded packets.
//! Matching runs outside cache locks; callers validate ownership before publishing the splice.

use crate::audio::player::ActiveDecoder;
use crate::cache::store::EncodedCacheChunk;
use ::sendspin::audio::AudioFormat;
use rustfft::{num_complex::Complex, FftPlanner};
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;

pub(crate) const MINIMUM_CACHE_DURATION_FOR_RECONNECT_ALIGNMENT: Duration = Duration::from_secs(30);
pub(crate) const RECONNECT_SEEK_OVERLAP: Duration = Duration::from_secs(5);
pub(crate) const PCM_REFERENCE_DURATION: Duration = Duration::from_secs(3);
pub(crate) const MAXIMUM_PCM_REFERENCE_DURATION: Duration = Duration::from_secs(10);
pub(crate) const MAXIMUM_ALIGNMENT_DURATION: Duration = Duration::from_secs(20);
pub(crate) const MAXIMUM_ALIGNMENT_BYTES: usize = 8 * 1024 * 1024;
const MINIMUM_MATCH_CORRELATION: f64 = 0.92;
const MINIMUM_MATCH_MARGIN: f64 = 0.02;

#[derive(Debug, Clone)]
pub(crate) struct PcmResumeReference {
    pub(crate) cache_identity: u64,
    pub(crate) end_frame: u64,
    pub(crate) format: AudioFormat,
    pub(crate) samples: Vec<i32>,
}

pub(crate) struct DecodedDownloadChunk {
    pub(crate) encoded: EncodedCacheChunk,
    pub(crate) samples: Arc<[i32]>,
}

pub(crate) enum StreamAppend {
    Waiting,
    Chunks(Vec<DecodedDownloadChunk>),
    RestartRequired,
}

struct PendingAlignment {
    reference: Arc<PcmResumeReference>,
    samples: Vec<i32>,
    chunks: Vec<DecodedDownloadChunk>,
    encoded_bytes: usize,
    next_match_frame: usize,
}

pub(crate) struct DownloadStream {
    decoder: ActiveDecoder,
    format: AudioFormat,
    next_frame: u64,
    alignment: Option<PendingAlignment>,
}

impl DownloadStream {
    // Stream configuration and replacement alignment.
    pub(crate) fn new(
        format: AudioFormat,
        start_millis: u64,
        reference: Option<Arc<PcmResumeReference>>,
    ) -> Result<Self, String> {
        let alignment = reference.map(|reference| PendingAlignment {
            next_match_frame: reference.samples.len() / usize::from(format.channels.max(1))
                + format.sample_rate as usize,
            reference,
            samples: Vec::new(),
            chunks: Vec::new(),
            encoded_bytes: 0,
        });
        Ok(Self {
            decoder: ActiveDecoder::from_format(&format)?,
            next_frame: start_millis.saturating_mul(u64::from(format.sample_rate)) / 1_000,
            format,
            alignment,
        })
    }

    // Decodes immediately; only a confirmed audio match permits replacement chunks to be published.
    pub(crate) fn receive(&mut self, data: Arc<[u8]>) -> Result<StreamAppend, String> {
        let samples = self.decoder.decode(&data)?;
        let channels = usize::from(self.format.channels.max(1));
        if samples.len() % channels != 0 {
            return Err("decoded audio contains an incomplete sample frame".to_string());
        }
        let frame_count = (samples.len() / channels) as u64;
        if frame_count == 0 {
            return Ok(StreamAppend::Waiting);
        }
        let chunk = DecodedDownloadChunk {
            encoded: EncodedCacheChunk {
                data,
                start_frame: self.next_frame,
                frame_count,
                leading_frames_to_skip: 0,
                sample_rate: self.format.sample_rate,
            },
            samples,
        };
        let Some(alignment) = self.alignment.as_mut() else {
            self.next_frame = self.next_frame.saturating_add(frame_count);
            return Ok(StreamAppend::Chunks(vec![chunk]));
        };
        if alignment.reference.format.codec != self.format.codec
            || alignment.reference.format.sample_rate != self.format.sample_rate
            || alignment.reference.format.channels != self.format.channels
            || alignment.reference.format.bit_depth != self.format.bit_depth
        {
            return Ok(StreamAppend::RestartRequired);
        }
        alignment.encoded_bytes = alignment
            .encoded_bytes
            .saturating_add(chunk.encoded.data.len());
        alignment.samples.extend_from_slice(&chunk.samples);
        alignment.chunks.push(chunk);
        let received_frames = alignment.samples.len() / channels;
        let limit_reached = alignment.encoded_bytes >= MAXIMUM_ALIGNMENT_BYTES
            || received_frames
                >= self.format.sample_rate as usize * MAXIMUM_ALIGNMENT_DURATION.as_secs() as usize;
        if received_frames < alignment.next_match_frame && !limit_reached {
            return Ok(StreamAppend::Waiting);
        }
        alignment.next_match_frame = received_frames + self.format.sample_rate as usize / 2;
        let Some(join_frame) = find_pcm_join(
            &alignment.reference.samples,
            &alignment.samples,
            self.format.sample_rate,
            self.format.channels,
        ) else {
            return Ok(if limit_reached {
                StreamAppend::RestartRequired
            } else {
                StreamAppend::Waiting
            });
        };
        let alignment = self.alignment.take().expect("matched alignment is present");
        let mut remaining_skip = join_frame as u64;
        let mut next_frame = alignment.reference.end_frame;
        let mut chunks = Vec::new();
        for mut chunk in alignment.chunks {
            if remaining_skip >= chunk.encoded.frame_count {
                remaining_skip -= chunk.encoded.frame_count;
                continue;
            }
            chunk.encoded.leading_frames_to_skip = remaining_skip;
            remaining_skip = 0;
            chunk.encoded.start_frame = next_frame;
            next_frame += chunk.encoded.audible_frames();
            chunks.push(chunk);
        }
        self.next_frame = next_frame;
        Ok(StreamAppend::Chunks(chunks))
    }

    // Stream completion queries.
    pub(crate) fn requires_alignment(&self) -> bool {
        self.alignment.is_some()
    }
}

#[derive(Clone)]
pub(crate) struct PcmTail {
    samples: VecDeque<i32>,
    channels: usize,
    maximum_samples: usize,
}

impl PcmTail {
    // Construction and retained reference management.
    pub(crate) fn new(sample_rate: u32, channels: u8) -> Self {
        let channels = usize::from(channels.max(1));
        Self {
            samples: VecDeque::new(),
            channels,
            maximum_samples: sample_rate as usize
                * MAXIMUM_PCM_REFERENCE_DURATION.as_secs() as usize
                * channels,
        }
    }

    pub(crate) fn append(&mut self, samples: &[i32]) {
        self.samples.extend(samples.iter().copied());
        let excess = self.samples.len().saturating_sub(self.maximum_samples);
        self.samples.drain(..excess);
    }

    pub(crate) fn reference(&self, sample_rate: u32) -> Vec<i32> {
        let normal_length =
            sample_rate as usize * PCM_REFERENCE_DURATION.as_secs() as usize * self.channels;
        let start = self.samples.len().saturating_sub(normal_length);
        let normal: Vec<_> = self.samples.iter().skip(start).copied().collect();
        if signal_variance(&normal) > 1.0 {
            normal
        } else {
            self.samples.iter().copied().collect()
        }
    }
}

// Sample-domain alignment.
// Returns the first new frame; correlation tolerates codec noise but rejects ambiguous repeats.
pub(crate) fn find_pcm_join(
    reference: &[i32],
    incoming: &[i32],
    sample_rate: u32,
    channels: u8,
) -> Option<usize> {
    let channels = usize::from(channels.max(1));
    let reference_frames = reference.len() / channels;
    let incoming_frames = incoming.len() / channels;
    if reference_frames == 0 || incoming_frames < reference_frames {
        return None;
    }
    let channel = (0..channels).max_by(|left, right| {
        channel_variance(reference, channels, *left)
            .total_cmp(&channel_variance(reference, channels, *right))
    })?;
    if channel_variance(reference, channels, channel) <= 1.0 {
        return None;
    }
    let stride = (sample_rate as usize / 12_000).max(1);
    let reference_signal = reduced_channel(reference, channels, channel, stride);
    let incoming_signal = reduced_channel(incoming, channels, channel, stride);
    if reference_signal.is_empty() || incoming_signal.len() < reference_signal.len() {
        return None;
    }
    let scores = correlation_scores(&reference_signal, &incoming_signal);
    let (best_offset, best_score) = scores
        .iter()
        .enumerate()
        .max_by(|left, right| left.1.total_cmp(right.1))?;
    if *best_score < 0.45 {
        return None;
    }
    let exclusion = (sample_rate as usize / 100 / stride).max(1);
    let competing_score = scores
        .iter()
        .enumerate()
        .filter(|(offset, _)| offset.abs_diff(best_offset) > exclusion)
        .map(|(_, score)| *score)
        .fold(-1.0_f64, f64::max);
    if *best_score - competing_score < MINIMUM_MATCH_MARGIN {
        return None;
    }
    let approximate_offset = best_offset * stride;
    let start = approximate_offset.saturating_sub(stride * 2);
    let end = (approximate_offset + stride * 2).min(incoming_frames - reference_frames);
    let (offset, score) = (start..=end)
        .map(|offset| {
            let samples = &incoming[offset * channels..(offset + reference_frames) * channels];
            (offset, normalized_correlation(reference, samples))
        })
        .max_by(|left, right| left.1.total_cmp(&right.1))?;
    let candidate = &incoming[offset * channels..(offset + reference_frames) * channels];
    (score >= MINIMUM_MATCH_CORRELATION && reference_windows_match(reference, candidate, channels))
        .then_some(offset + reference_frames)
}

// Correlation and signal reduction.
fn reference_windows_match(reference: &[i32], candidate: &[i32], channels: usize) -> bool {
    let frames = reference.len() / channels;
    (0..3).all(|window| {
        let start = frames * window / 3 * channels;
        let end = frames * (window + 1) / 3 * channels;
        let reference_window = &reference[start..end];
        let candidate_window = &candidate[start..end];
        if signal_variance(reference_window) <= 1.0 {
            signal_variance(candidate_window) <= 1.0
        } else {
            normalized_correlation(reference_window, candidate_window) >= MINIMUM_MATCH_CORRELATION
        }
    })
}

fn reduced_channel(samples: &[i32], channels: usize, channel: usize, stride: usize) -> Vec<f64> {
    samples
        .chunks_exact(channels * stride)
        .map(|block| {
            block
                .chunks_exact(channels)
                .map(|frame| frame[channel] as f64)
                .sum::<f64>()
                / stride as f64
        })
        .collect()
}

fn signal_variance(samples: &[i32]) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    let mean = samples.iter().map(|sample| *sample as f64).sum::<f64>() / samples.len() as f64;
    samples
        .iter()
        .map(|sample| (*sample as f64 - mean).powi(2))
        .sum::<f64>()
        / samples.len() as f64
}

fn channel_variance(samples: &[i32], channels: usize, channel: usize) -> f64 {
    let signal: Vec<_> = samples
        .chunks_exact(channels)
        .map(|frame| frame[channel])
        .collect();
    signal_variance(&signal)
}

fn normalized_correlation(reference: &[i32], candidate: &[i32]) -> f64 {
    let length = reference.len() as f64;
    let reference_mean = reference.iter().map(|sample| *sample as f64).sum::<f64>() / length;
    let candidate_mean = candidate.iter().map(|sample| *sample as f64).sum::<f64>() / length;
    let mut product = 0.0;
    let mut reference_energy = 0.0;
    let mut candidate_energy = 0.0;
    for (reference_sample, candidate_sample) in reference.iter().zip(candidate) {
        let reference_value = *reference_sample as f64 - reference_mean;
        let candidate_value = *candidate_sample as f64 - candidate_mean;
        product += reference_value * candidate_value;
        reference_energy += reference_value * reference_value;
        candidate_energy += candidate_value * candidate_value;
    }
    let denominator = (reference_energy * candidate_energy).sqrt();
    if denominator > 0.0 {
        product / denominator
    } else {
        -1.0
    }
}

fn correlation_scores(reference: &[f64], incoming: &[f64]) -> Vec<f64> {
    let length = reference.len();
    let mean = reference.iter().sum::<f64>() / length as f64;
    let energy = reference
        .iter()
        .map(|sample| (sample - mean).powi(2))
        .sum::<f64>();
    let fft_length = (reference.len() + incoming.len() - 1).next_power_of_two();
    let mut reference_spectrum = vec![Complex::new(0.0, 0.0); fft_length];
    let mut incoming_spectrum = reference_spectrum.clone();
    for (target, sample) in reference_spectrum.iter_mut().zip(reference.iter().rev()) {
        target.re = sample - mean;
    }
    for (target, sample) in incoming_spectrum.iter_mut().zip(incoming) {
        target.re = *sample;
    }
    let mut planner = FftPlanner::<f64>::new();
    let forward = planner.plan_fft_forward(fft_length);
    forward.process(&mut reference_spectrum);
    forward.process(&mut incoming_spectrum);
    for (reference_value, incoming_value) in reference_spectrum.iter_mut().zip(incoming_spectrum) {
        *reference_value *= incoming_value;
    }
    planner
        .plan_fft_inverse(fft_length)
        .process(&mut reference_spectrum);
    let mut sums = vec![0.0; incoming.len() + 1];
    let mut squares = sums.clone();
    for (index, sample) in incoming.iter().enumerate() {
        sums[index + 1] = sums[index] + sample;
        squares[index + 1] = squares[index] + sample * sample;
    }
    (0..=incoming.len() - length)
        .map(|offset| {
            let sum = sums[offset + length] - sums[offset];
            let candidate_energy =
                (squares[offset + length] - squares[offset] - sum * sum / length as f64).max(0.0);
            let denominator = (energy * candidate_energy).sqrt();
            if denominator > 0.0 {
                reference_spectrum[offset + length - 1].re / fft_length as f64 / denominator
            } else {
                -1.0
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn signal(frames: usize) -> Vec<i32> {
        let mut state = 17_u32;
        (0..frames)
            .map(|_| {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (state >> 8) as i32 - 8_000_000
            })
            .collect()
    }

    #[test]
    fn finds_sample_exact_join_across_arbitrary_chunks() {
        let incoming = signal(8_000);
        let reference = incoming[2_137..5_137].to_vec();
        assert_eq!(find_pcm_join(&reference, &incoming, 1_000, 1), Some(5_137));
    }

    #[test]
    fn tolerates_gain_and_small_codec_noise() {
        let original = signal(8_000);
        let incoming: Vec<_> = original
            .iter()
            .enumerate()
            .map(|(index, sample)| sample / 2 + (index % 101) as i32)
            .collect();
        assert_eq!(
            find_pcm_join(&original[2_137..5_137], &incoming, 1_000, 1),
            Some(5_137)
        );
    }

    #[test]
    fn rejects_silence_and_repeated_matches() {
        assert_eq!(
            find_pcm_join(&vec![0; 3_000], &vec![0; 8_000], 1_000, 1),
            None
        );
        let reference = signal(3_000);
        let mut incoming = reference.clone();
        incoming.extend(&reference);
        assert_eq!(find_pcm_join(&reference, &incoming, 1_000, 1), None);
    }

    #[test]
    fn tail_retains_only_bounded_accepted_audio() {
        let mut tail = PcmTail::new(1_000, 1);
        tail.append(&signal(15_000));
        assert_eq!(tail.samples.len(), 10_000);
        assert_eq!(tail.reference(1_000).len(), 3_000);
    }

    #[test]
    fn finds_full_rate_offset_between_reduced_signal_frames() {
        let incoming = signal(350_000);
        assert_eq!(
            find_pcm_join(&incoming[91_137..235_137], &incoming, 48_000, 1),
            Some(235_137)
        );
    }

    fn pcm_format() -> AudioFormat {
        AudioFormat {
            codec: ::sendspin::audio::Codec::Pcm,
            sample_rate: 1_000,
            channels: 1,
            bit_depth: 16,
            codec_header: None,
        }
    }

    fn encoded(samples: &[i32]) -> Arc<[u8]> {
        samples
            .iter()
            .flat_map(|sample| ((*sample >> 9) as i16).to_le_bytes())
            .collect::<Vec<_>>()
            .into()
    }

    #[test]
    fn stream_discards_duplicates_and_marks_only_the_transition() {
        let incoming = signal(8_000);
        let decoded = ActiveDecoder::from_format(&pcm_format())
            .unwrap()
            .decode(&encoded(&incoming))
            .unwrap();
        let reference = Arc::new(PcmResumeReference {
            cache_identity: 1,
            end_frame: 35_000,
            format: pcm_format(),
            samples: decoded[2_137..5_137].to_vec(),
        });
        let mut stream = DownloadStream::new(pcm_format(), 30_000, Some(reference)).unwrap();
        let mut accepted = Vec::new();
        for samples in incoming.chunks(500) {
            if let StreamAppend::Chunks(chunks) = stream.receive(encoded(samples)).unwrap() {
                accepted.extend(chunks);
            }
        }
        assert!(!stream.requires_alignment());
        assert_eq!(accepted[0].encoded.start_frame, 35_000);
        assert_eq!(accepted[0].encoded.leading_frames_to_skip, 137);
        assert_eq!(
            accepted
                .iter()
                .map(|chunk| chunk.encoded.audible_frames())
                .sum::<u64>(),
            2_863
        );
        assert!(accepted[1..]
            .iter()
            .all(|chunk| chunk.encoded.leading_frames_to_skip == 0));
    }

    #[test]
    fn unmatched_stream_requests_a_fresh_download_at_the_limit() {
        let reference = Arc::new(PcmResumeReference {
            cache_identity: 1,
            end_frame: 35_000,
            format: pcm_format(),
            samples: signal(3_000),
        });
        let mut stream = DownloadStream::new(pcm_format(), 30_000, Some(reference)).unwrap();
        assert!(matches!(
            stream.receive(encoded(&vec![0; 20_000])).unwrap(),
            StreamAppend::RestartRequired
        ));
    }
}
