//! Each encoded chunk is a complete FLAC or Ogg-Opus file with independent codec state.
//! Decoding is bounded to one declared chunk, including Opus pre-skip and end trimming.

use super::models::{AudioCodec, AudioFormat};
use crate::audio::buffer::EncodedChunk;
use ffmpeg_next as ffmpeg;
use std::io::Cursor;
use std::sync::{Arc, OnceLock};

static FFMPEG_INITIALIZED: OnceLock<Result<(), String>> = OnceLock::new();

pub(crate) fn decode_chunk(
    chunk: &EncodedChunk,
    format: &AudioFormat,
) -> Result<Arc<[i32]>, String> {
    FFMPEG_INITIALIZED
        .get_or_init(|| {
            ffmpeg::init().map_err(|error| format!("FFmpeg initialization failed: {error:?}"))
        })
        .as_ref()
        .map_err(Clone::clone)?;
    format.validate()?;
    if chunk.payload_offset >= chunk.encoded_bytes.len() {
        return Err("encoded audio payload is empty or out of range".into());
    }
    if chunk.frame_count > format.sample_rate_hz as u64 || chunk.frame_count == 0 {
        return Err("invalid chunk frame count".into());
    }
    let samples = decode_audio(chunk, format)
        .map_err(|error| format!("FFmpeg audio decode failed: {error}"))?;
    if samples.len() != chunk.frame_count as usize * format.channels as usize {
        return Err("decoded frame count differs from its header".into());
    }
    Ok(samples.into())
}

fn decode_audio(chunk: &EncodedChunk, format: &AudioFormat) -> Result<Vec<i32>, ffmpeg::Error> {
    let io =
        ffmpeg::format::context::StreamIo::from_read_seek(Cursor::new(chunk.payload().to_vec()))?;
    let mut options = ffmpeg::Dictionary::new();
    options.set("format_whitelist", "flac,ogg");
    options.set("codec_whitelist", "flac,opus");
    options.set("err_detect", "crccheck+bitstream+explode");
    let mut input = ffmpeg::format::input_from_stream(io, None, Some(options))?;
    if input.nb_streams() != 1 {
        return Err(ffmpeg::Error::InvalidData);
    }
    let stream = input.stream(0).ok_or(ffmpeg::Error::StreamNotFound)?;
    let parameters = stream.parameters();
    let expected_codec = match format.codec {
        AudioCodec::Flac => ffmpeg::codec::Id::FLAC,
        AudioCodec::Opus => ffmpeg::codec::Id::OPUS,
    };
    if parameters.id() != expected_codec
        || (format.codec == AudioCodec::Flac
            && unsafe { (*parameters.as_ptr()).bits_per_raw_sample } != 16)
    {
        return Err(ffmpeg::Error::InvalidData);
    }
    let mut context = ffmpeg::codec::context::Context::from_parameters(parameters)?.decoder();
    context.set_packet_time_base(stream.time_base());
    context.check(ffmpeg::codec::decoder::Check::CRC | ffmpeg::codec::decoder::Check::EXPLODE);
    let mut decoder = context.audio()?;
    if decoder.rate() != format.sample_rate_hz || decoder.channels() != u16::from(format.channels) {
        return Err(ffmpeg::Error::InvalidData);
    }
    let mut converter = ffmpeg::software::resampling::Context::get(
        decoder.format(),
        decoder.channel_layout(),
        decoder.rate(),
        ffmpeg::format::Sample::I32(ffmpeg::format::sample::Type::Packed),
        decoder.channel_layout(),
        decoder.rate(),
    )?;
    let expected = chunk.frame_count as usize * usize::from(format.channels);
    let mut samples = Vec::with_capacity(expected);
    loop {
        let mut packet = ffmpeg::Packet::empty();
        match packet.read(&mut input) {
            Ok(()) => {
                if packet.stream() != 0 || packet.is_corrupt() {
                    return Err(ffmpeg::Error::InvalidData);
                }
                decoder.send_packet(&packet)?;
                receive_samples(&mut decoder, &mut converter, format, expected, &mut samples)?;
            }
            Err(ffmpeg::Error::Eof) => break,
            Err(error) => return Err(error),
        }
    }
    decoder.send_eof()?;
    receive_samples(&mut decoder, &mut converter, format, expected, &mut samples)?;
    Ok(samples)
}

fn receive_samples(
    decoder: &mut ffmpeg::decoder::Audio,
    converter: &mut ffmpeg::software::resampling::Context,
    format: &AudioFormat,
    expected: usize,
    samples: &mut Vec<i32>,
) -> Result<(), ffmpeg::Error> {
    loop {
        let mut decoded = ffmpeg::frame::Audio::empty();
        match decoder.receive_frame(&mut decoded) {
            Ok(()) => {
                if decoded.samples() == 0 {
                    continue;
                }
                if decoded.rate() != format.sample_rate_hz
                    || decoded.channels() != u16::from(format.channels)
                    || decoded.format() != converter.input().format
                    || decoded.channel_layout() != converter.input().channel_layout
                    || samples.len() + decoded.samples() * usize::from(format.channels) > expected
                {
                    return Err(ffmpeg::Error::InvalidData);
                }
                let mut converted = ffmpeg::frame::Audio::empty();
                if converter.run(&decoded, &mut converted)?.is_some() {
                    return Err(ffmpeg::Error::InvalidData);
                }
                if converted.samples() > 0 {
                    samples.extend_from_slice(converted.plane::<i32>(0));
                }
            }
            Err(ffmpeg::Error::Eof) => return Ok(()),
            Err(ffmpeg::Error::Other { errno }) if errno == ffmpeg::error::EAGAIN => return Ok(()),
            Err(error) => return Err(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::models::test_format;
    use base64::Engine;

    #[derive(serde::Deserialize)]
    struct Fixture {
        name: String,
        codec: String,
        rate: u32,
        channels: u8,
        frames: u64,
        history: u64,
        payload: String,
    }

    fn fixtures() -> Vec<Fixture> {
        serde_json::from_str(include_str!("../../tests/fixtures/audio.json")).unwrap()
    }

    fn fixture_chunk(fixture: &Fixture) -> EncodedChunk {
        chunk(
            base64::engine::general_purpose::STANDARD
                .decode(&fixture.payload)
                .unwrap(),
            fixture.frames,
        )
    }

    fn fixture_format(fixture: &Fixture) -> AudioFormat {
        AudioFormat {
            codec: if fixture.codec == "flac" {
                AudioCodec::Flac
            } else {
                AudioCodec::Opus
            },
            sample_rate_hz: fixture.rate,
            channels: fixture.channels,
            bit_depth: (fixture.codec == "flac").then_some(16),
        }
    }

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
    fn decodes_synthetic_reference_profiles_without_artifacts() {
        for fixture in fixtures() {
            let encoded = fixture_chunk(&fixture);
            let format = fixture_format(&fixture);
            let samples = decode_chunk(&encoded, &format)
                .unwrap_or_else(|error| panic!("{}: {error}", fixture.name));
            assert_eq!(
                samples.len(),
                fixture.frames as usize * usize::from(fixture.channels),
                "{}",
                fixture.name
            );
            if fixture.codec == "flac" {
                for (index, sample) in samples.iter().enumerate() {
                    let frame = index / usize::from(fixture.channels);
                    let channel = index % usize::from(fixture.channels);
                    let expected = ((frame * 73 + channel * 997) % 50000) as i32 - 25000;
                    assert_eq!(*sample, expected << 16, "{} sample {index}", fixture.name);
                }
            } else {
                let mut signal = 0.0;
                let mut noise = 0.0;
                for (index, sample) in samples.iter().enumerate() {
                    let frame = index / 2 + fixture.history as usize;
                    let (frequency, amplitude) = if index % 2 == 0 {
                        (440.0, 0.2)
                    } else {
                        (997.0, 0.15)
                    };
                    let reference = amplitude
                        * (frame as f64 * frequency * std::f64::consts::TAU / 48000.0).sin();
                    let decoded = f64::from(*sample) / 2147483648.0;
                    signal += reference * reference;
                    noise += (decoded - reference).powi(2);
                }
                let snr = 10.0 * (signal / noise).log10();
                assert!(snr > 35.0, "{}: SNR {snr:.2} dB", fixture.name);
            }
        }
    }

    #[test]
    fn rejects_truncated_audio_and_incorrect_frame_counts() {
        for fixture in fixtures() {
            let format = fixture_format(&fixture);
            let mut encoded = fixture_chunk(&fixture);
            encoded.frame_count -= 1;
            assert!(
                decode_chunk(&encoded, &format).is_err(),
                "{} short header",
                fixture.name
            );
            encoded.frame_count += 2;
            assert!(
                decode_chunk(&encoded, &format).is_err(),
                "{} long header",
                fixture.name
            );
            encoded.frame_count = fixture.frames;
            encoded.encoded_bytes = encoded.encoded_bytes[..encoded.encoded_bytes.len() / 2].into();
            assert!(
                decode_chunk(&encoded, &format).is_err(),
                "{} truncated",
                fixture.name
            );
        }
    }

    #[test]
    fn keeps_codec_state_local_and_ignores_wire_header_bytes() {
        for fixture in fixtures() {
            let format = fixture_format(&fixture);
            let mut encoded = fixture_chunk(&fixture);
            let first = decode_chunk(&encoded, &format).unwrap();
            let mut prefixed = vec![0xff; 41];
            prefixed.extend_from_slice(encoded.payload());
            encoded.encoded_bytes = prefixed.into();
            encoded.payload_offset = 41;
            assert_eq!(
                decode_chunk(&encoded, &format).unwrap(),
                first,
                "{}",
                fixture.name
            );
        }
    }

    #[test]
    fn rejects_codec_rate_and_channel_mismatches() {
        let fixture = fixtures()
            .into_iter()
            .find(|fixture| fixture.name == "flac_48000_2")
            .unwrap();
        let encoded = fixture_chunk(&fixture);
        let mut format = fixture_format(&fixture);
        format.channels = 1;
        assert!(decode_chunk(&encoded, &format).is_err());
        format.channels = 2;
        format.sample_rate_hz = 44100;
        assert!(decode_chunk(&encoded, &format).is_err());
        format.sample_rate_hz = 48000;
        format.codec = AudioCodec::Opus;
        format.bit_depth = None;
        assert!(decode_chunk(&encoded, &format).is_err());
    }
}
