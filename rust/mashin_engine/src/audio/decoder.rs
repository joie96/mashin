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
