//! Each encoded chunk is a complete FLAC or Ogg-Opus file with independent codec state.
//! Decoding is bounded to one declared chunk, including Opus pre-skip and end trimming.

use super::models::{AudioCodec, AudioFormat};
use crate::audio::buffer::EncodedChunk;
use flac_codec::decode::{FlacSampleReader, Metadata};
use std::io::Cursor;
use std::sync::Arc;

pub(crate) fn decode_chunk(
    chunk: &EncodedChunk,
    format: &AudioFormat,
) -> Result<Arc<[i32]>, String> {
    format.validate()?;
    if chunk.payload_offset >= chunk.encoded_bytes.len() {
        return Err("encoded audio payload is empty or out of range".into());
    }
    if chunk.frame_count > format.sample_rate_hz as u64 || chunk.frame_count == 0 {
        return Err("invalid chunk frame count".into());
    }
    let samples = match format.codec {
        AudioCodec::Flac => decode_flac(chunk, format)?,
        AudioCodec::Opus => decode_opus(chunk, format)?,
    };
    if samples.len() != chunk.frame_count as usize * format.channels as usize {
        return Err("decoded frame count differs from its header".into());
    }
    Ok(samples.into())
}

fn decode_flac(chunk: &EncodedChunk, format: &AudioFormat) -> Result<Vec<i32>, String> {
    let mut reader = FlacSampleReader::new(chunk.payload())
        .map_err(|error| format!("invalid FLAC stream: {error}"))?;
    if reader.sample_rate() != format.sample_rate_hz
        || reader.channel_count() != format.channels
        || reader.bits_per_sample() != 16
    {
        return Err("FLAC format differs from descriptor".into());
    }
    let expected = chunk.frame_count as usize * format.channels as usize;
    let mut samples = Vec::with_capacity(expected);
    let mut scratch = [0i32; 4096];
    loop {
        let count = reader
            .read(&mut scratch)
            .map_err(|error| format!("FLAC decode failed: {error}"))?;
        if count == 0 {
            break;
        }
        if samples.len() + count > expected {
            return Err("FLAC exceeds chunk duration".into());
        }
        samples.extend(
            scratch[..count]
                .iter()
                .map(|sample| sample.wrapping_shl(16)),
        );
    }
    Ok(samples)
}

fn decode_opus(chunk: &EncodedChunk, format: &AudioFormat) -> Result<Vec<i32>, String> {
    let mut reader = ogg::PacketReader::new(Cursor::new(chunk.payload()));
    let header = reader
        .read_packet_expected()
        .map_err(|error| format!("invalid Ogg Opus header: {error}"))?;
    let bytes = &header.data;
    if bytes.len() != 19
        || &bytes[..8] != b"OpusHead"
        || bytes[8] != 1
        || bytes[9] != format.channels
        || bytes[16..18] != [0, 0]
        || bytes[18] != 0
    {
        return Err("unsupported Opus identification header".into());
    }
    let pre_skip = u16::from_le_bytes(bytes[10..12].try_into().unwrap()) as usize;
    let serial = header.stream_serial();
    let tags = reader
        .read_packet_expected()
        .map_err(|error| format!("missing Opus tags: {error}"))?;
    if tags.stream_serial() != serial || !tags.data.starts_with(b"OpusTags") {
        return Err("invalid Opus comment header".into());
    }
    let channels = format.channels as usize;
    let expected_frames = chunk.frame_count as usize;
    let maximum_frames = expected_frames + pre_skip + 960;
    let mut samples = Vec::with_capacity(maximum_frames * channels);
    let mut scratch = vec![0f32; 960 * channels];
    let mut decoder = opus_rs::OpusDecoder::new(48000, channels).map_err(str::to_string)?;
    let mut final_granule = None;
    while let Some(packet) = reader
        .read_packet()
        .map_err(|error| format!("invalid Ogg Opus packet: {error}"))?
    {
        if packet.stream_serial() != serial || final_granule.is_some() {
            return Err("unexpected chained Opus stream".into());
        }
        let frames = decoder
            .decode(&packet.data, 960, &mut scratch)
            .map_err(|error| format!("Opus decode failed: {error}"))?;
        if frames != 960 || samples.len() / channels + frames > maximum_frames {
            return Err("Opus exceeds chunk duration".into());
        }
        for sample in &scratch[..frames * channels] {
            if !sample.is_finite() {
                return Err("Opus returned non-finite audio".into());
            }
            samples.push((sample.clamp(-1.0, 1.0) as f64 * i32::MAX as f64) as i32);
        }
        if packet.last_in_stream() {
            final_granule = Some(packet.absgp_page());
        }
    }
    let final_granule = final_granule.ok_or("Opus chunk has no final granule")?;
    if final_granule != (pre_skip + expected_frames) as u64
        || final_granule > (samples.len() / channels) as u64
    {
        return Err("Opus granule differs from declared frames".into());
    }
    Ok(samples[pre_skip * channels..final_granule as usize * channels].to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::models::test_format;

    fn chunk(payload: Vec<u8>, frames: u64) -> EncodedChunk {
        EncodedChunk {
            stream_id: uuid::Uuid::new_v4(),
            index: 1,
            first_frame: 0,
            frame_count: frames,
            is_final: true,
            encoded_bytes: payload.into(),
            payload_offset: 0,
        }
    }

    #[test]
    fn rejects_invalid_encoded_audio_without_rendering() {
        assert!(decode_chunk(&chunk(vec![1, 2, 3], 10), &test_format()).is_err());
        let mut invalid = chunk(vec![1], 10);
        invalid.payload_offset = 2;
        assert!(decode_chunk(&invalid, &test_format()).is_err());
        assert!(decode_chunk(&chunk(Vec::new(), 10), &test_format()).is_err());
    }

    #[test]
    fn opus_256k_audio_packet_decodes_as_twenty_milliseconds() {
        let mut encoder = opus_rs::OpusEncoder::new(48000, 2, opus_rs::Application::Audio).unwrap();
        encoder.bitrate_bps = 256000;
        let pcm: Vec<f32> = (0..960)
            .flat_map(|frame| {
                let sample = (frame as f32 * 440.0 * std::f32::consts::TAU / 48000.0).sin() * 0.25;
                [sample, -sample]
            })
            .collect();
        let mut packet = vec![0u8; 1276];
        let size = encoder.encode(&pcm, 960, &mut packet).unwrap();
        packet.truncate(size);
        assert_eq!(packet[0] & 3, 0);
        let mut writer = ogg::PacketWriter::new(Vec::new());
        let mut header = b"OpusHead".to_vec();
        header.extend_from_slice(&[1, 2]);
        header.extend_from_slice(&0u16.to_le_bytes());
        header.extend_from_slice(&48000u32.to_le_bytes());
        header.extend_from_slice(&[0, 0, 0]);
        writer
            .write_packet(header, 1, ogg::PacketWriteEndInfo::EndPage, 0)
            .unwrap();
        let mut tags = b"OpusTags".to_vec();
        tags.extend_from_slice(&[0; 8]);
        writer
            .write_packet(tags, 1, ogg::PacketWriteEndInfo::EndPage, 0)
            .unwrap();
        writer
            .write_packet(packet, 1, ogg::PacketWriteEndInfo::EndStream, 960)
            .unwrap();
        let format = AudioFormat {
            codec: AudioCodec::Opus,
            sample_rate_hz: 48000,
            bit_depth: None,
            channels: 2,
        };
        let samples = decode_chunk(&chunk(writer.into_inner(), 960), &format).unwrap();
        assert_eq!(samples.len(), 1920);
        assert!(samples.iter().any(|sample| *sample != 0));
    }

    #[test]
    fn ogg_opus_honors_pre_skip_and_final_granule() {
        let mut writer = ogg::PacketWriter::new(Vec::new());
        let mut header = b"OpusHead".to_vec();
        header.extend_from_slice(&[1, 2]);
        header.extend_from_slice(&312u16.to_le_bytes());
        header.extend_from_slice(&48000u32.to_le_bytes());
        header.extend_from_slice(&[0, 0, 0]);
        writer
            .write_packet(header, 1, ogg::PacketWriteEndInfo::EndPage, 0)
            .unwrap();
        let mut tags = b"OpusTags".to_vec();
        tags.extend_from_slice(&[0; 8]);
        writer
            .write_packet(tags, 1, ogg::PacketWriteEndInfo::EndPage, 0)
            .unwrap();
        writer
            .write_packet(
                vec![0xfc, 0xff, 0xfe],
                1,
                ogg::PacketWriteEndInfo::EndStream,
                900,
            )
            .unwrap();
        let chunk = chunk(writer.into_inner(), 588);
        let format = AudioFormat {
            codec: AudioCodec::Opus,
            sample_rate_hz: 48000,
            bit_depth: None,
            channels: 2,
        };
        let samples = decode_chunk(&chunk, &format).unwrap();
        assert_eq!(samples.len(), 588 * 2);
        assert!(decode_chunk(
            &EncodedChunk {
                frame_count: 589,
                ..chunk
            },
            &format
        )
        .is_err());
    }
}
