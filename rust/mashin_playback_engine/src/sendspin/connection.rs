use ::sendspin::audio::AudioFormat;
use ::sendspin::protocol::messages::{AudioFormatSpec, PlayerV1Support};

pub(crate) fn supported_audio_formats(preferred_codec: &str) -> Vec<AudioFormatSpec> {
    let preferred = preferred_codec.trim().to_ascii_lowercase();
    let codecs = [preferred.as_str(), "opus", "flac", "pcm"];
    let sample_rates = [48_000, 44_100];
    let mut formats = Vec::new();

    for codec in codecs {
        if codec.is_empty()
            || formats
                .iter()
                .any(|format: &AudioFormatSpec| format.codec == codec)
        {
            continue;
        }
        for sample_rate in sample_rates {
            formats.push(AudioFormatSpec {
                codec: codec.to_string(),
                sample_rate,
                channels: 2,
                bit_depth: 16,
            });
        }
    }

    formats
}

pub(crate) fn player_support(preferred_codec: &str, buffer_capacity: u64) -> PlayerV1Support {
    PlayerV1Support {
        supported_formats: supported_audio_formats(preferred_codec),
        buffer_capacity: buffer_capacity.min(u32::MAX as u64) as u32,
        supported_commands: Vec::new(),
    }
}

pub(crate) fn audio_format_from_stream(
    codec: &str,
    sample_rate: u32,
    channels: u8,
    bit_depth: u8,
    codec_header: Option<&str>,
) -> Result<AudioFormat, String> {
    let codec = match codec.trim().to_ascii_lowercase().as_str() {
        "pcm" => ::sendspin::audio::Codec::Pcm,
        "opus" => ::sendspin::audio::Codec::Opus,
        "flac" => ::sendspin::audio::Codec::Flac,
        value => return Err(format!("unsupported Sendspin codec: {value}")),
    };
    let codec_header = codec_header
        .map(|header| base64::Engine::decode(&base64::engine::general_purpose::STANDARD, header))
        .transpose()
        .map_err(|error| format!("invalid Sendspin codec header: {error}"))?;
    Ok(AudioFormat {
        codec,
        sample_rate,
        channels,
        bit_depth,
        codec_header,
    })
}
