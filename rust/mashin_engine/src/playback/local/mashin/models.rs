//! Immutable V1 stream identities and formats shared by transport, cache, and decoder.

use crate::music_assistant::models::identifiers::{PlayerId, QueueId, QueueItemId};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub(crate) const MAX_INTEGER: u64 = 9_007_199_254_740_991;
pub(crate) const HEADER_BYTES: usize = 41;
pub(crate) const MAX_PAYLOAD_BYTES: usize = 262_144;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AudioProfileId {
    #[default]
    Flac,
    #[serde(rename = "opus_96k")]
    Opus96k,
    #[serde(rename = "opus_128k")]
    Opus128k,
    #[serde(rename = "opus_192k")]
    Opus192k,
    #[serde(rename = "opus_256k")]
    Opus256k,
}

impl AudioProfileId {
    pub(crate) const ALL: [Self; 5] = [
        Self::Flac,
        Self::Opus96k,
        Self::Opus128k,
        Self::Opus192k,
        Self::Opus256k,
    ];

    pub(crate) fn bitrate(self) -> Option<u32> {
        match self {
            Self::Flac => None,
            Self::Opus96k => Some(96_000),
            Self::Opus128k => Some(128_000),
            Self::Opus192k => Some(192_000),
            Self::Opus256k => Some(256_000),
        }
    }
}

impl std::str::FromStr for AudioProfileId {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "flac" => Ok(Self::Flac),
            "opus_96k" => Ok(Self::Opus96k),
            "opus_128k" => Ok(Self::Opus128k),
            "opus_192k" => Ok(Self::Opus192k),
            "opus_256k" => Ok(Self::Opus256k),
            _ => Err("invalid Mashin audio profile".into()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum OutputState {
    Idle,
    Buffering,
    Playing,
    Paused,
    Ended,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Container {
    Flac,
    Ogg,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Codec {
    Flac,
    Opus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct OutputFormat {
    pub(crate) profile_id: AudioProfileId,
    pub(crate) container: Container,
    pub(crate) codec: Codec,
    pub(crate) sample_rate_hz: u32,
    pub(crate) bit_depth: Option<u8>,
    pub(crate) channels: u8,
    pub(crate) target_bitrate_bps: Option<u32>,
    pub(crate) uses_vbr: bool,
}

impl OutputFormat {
    // Protocol-specific profile validation stays at the receiver boundary.
    pub(crate) fn audio_format(&self) -> crate::audio::models::AudioFormat {
        crate::audio::models::AudioFormat {
            codec: match self.codec {
                Codec::Flac => crate::audio::models::AudioCodec::Flac,
                Codec::Opus => crate::audio::models::AudioCodec::Opus,
            },
            sample_rate_hz: self.sample_rate_hz,
            channels: self.channels,
            bit_depth: self.bit_depth,
        }
    }

    pub(crate) fn validate(&self) -> Result<(), String> {
        let valid = match self.profile_id {
            AudioProfileId::Flac => {
                self.container == Container::Flac
                    && self.codec == Codec::Flac
                    && matches!(self.sample_rate_hz, 44_100 | 48_000)
                    && self.bit_depth == Some(16)
                    && matches!(self.channels, 1 | 2)
                    && self.target_bitrate_bps.is_none()
                    && !self.uses_vbr
            }
            profile => {
                self.container == Container::Ogg
                    && self.codec == Codec::Opus
                    && self.sample_rate_hz == 48_000
                    && self.bit_depth.is_none()
                    && self.channels == 2
                    && self.target_bitrate_bps == profile.bitrate()
                    && self.uses_vbr
            }
        };
        if valid {
            Ok(())
        } else {
            Err("invalid Mashin output format".into())
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PlaybackIdentity {
    pub(crate) player_id: PlayerId,
    pub(crate) queue_id: QueueId,
    pub(crate) queue_item_id: QueueItemId,
    pub(crate) queue_session_id: String,
    pub(crate) playback_generation: u64,
    pub(crate) playback_instance_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StreamDescriptor {
    #[serde(deserialize_with = "deserialize_stream_id")]
    pub(crate) stream_id: Uuid,
    pub(crate) playback: PlaybackIdentity,
    pub(crate) output_format: OutputFormat,
    pub(crate) media_start_position_millis: u64,
    pub(crate) duration_millis: Option<u64>,
}

impl StreamDescriptor {
    pub(crate) fn validate(&self) -> Result<(), String> {
        self.output_format.validate()?;
        if self.stream_id.get_version_num() != 4
            || self.stream_id.get_variant() != uuid::Variant::RFC4122
            || self.playback.playback_generation > MAX_INTEGER
            || self.media_start_position_millis > MAX_INTEGER
            || self
                .duration_millis
                .is_some_and(|duration| duration > MAX_INTEGER)
            || self.playback.player_id.0.is_empty()
            || self.playback.queue_id.0.is_empty()
            || self.playback.queue_item_id.0.is_empty()
            || self.playback.queue_session_id.is_empty()
            || self.playback.playback_instance_id.is_empty()
        {
            return Err("invalid Mashin stream descriptor".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StreamCursor {
    #[serde(deserialize_with = "deserialize_stream_id")]
    pub(crate) stream_id: Uuid,
    pub(crate) next_chunk_index: u64,
}

fn deserialize_stream_id<'de, Deserializer: serde::Deserializer<'de>>(
    deserializer: Deserializer,
) -> Result<Uuid, Deserializer::Error> {
    let value = String::deserialize(deserializer)?;
    let identifier = Uuid::parse_str(&value).map_err(serde::de::Error::custom)?;
    if identifier.get_version_num() != 4
        || identifier.get_variant() != uuid::Variant::RFC4122
        || identifier.to_string() != value
    {
        return Err(serde::de::Error::custom(
            "Mashin requires a canonical UUIDv4",
        ));
    }
    Ok(identifier)
}

#[cfg(test)]
pub(crate) fn test_stream() -> StreamDescriptor {
    StreamDescriptor {
        stream_id: Uuid::new_v4(),
        playback: PlaybackIdentity {
            player_id: PlayerId("player".into()),
            queue_id: QueueId("queue".into()),
            queue_item_id: QueueItemId("item".into()),
            queue_session_id: "session".into(),
            playback_generation: 1,
            playback_instance_id: "instance".into(),
        },
        output_format: OutputFormat {
            profile_id: AudioProfileId::Flac,
            container: Container::Flac,
            codec: Codec::Flac,
            sample_rate_hz: 48000,
            bit_depth: Some(16),
            channels: 2,
            target_bitrate_bps: None,
            uses_vbr: false,
        },
        media_start_position_millis: 0,
        duration_millis: Some(180000),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_ids_match_the_provider_contract() {
        assert_eq!(
            serde_json::to_value(AudioProfileId::ALL).unwrap(),
            serde_json::json!(["flac", "opus_96k", "opus_128k", "opus_192k", "opus_256k"])
        );
        for profile in AudioProfileId::ALL {
            let wire_name = serde_json::to_value(profile).unwrap();
            assert_eq!(
                wire_name
                    .as_str()
                    .unwrap()
                    .parse::<AudioProfileId>()
                    .unwrap(),
                profile
            );
        }
        assert!("opus".parse::<AudioProfileId>().is_err());
    }

    #[test]
    fn validates_complete_profile_formats() {
        let mut stream = test_stream();
        assert!(stream.validate().is_ok());
        stream.output_format.channels = 3;
        assert!(stream.validate().is_err());
        stream.output_format = OutputFormat {
            profile_id: AudioProfileId::Opus128k,
            container: Container::Ogg,
            codec: Codec::Opus,
            sample_rate_hz: 48000,
            bit_depth: None,
            channels: 2,
            target_bitrate_bps: Some(128000),
            uses_vbr: true,
        };
        assert!(stream.validate().is_ok());
        stream.output_format.bit_depth = Some(16);
        assert!(stream.validate().is_err());
    }

    #[test]
    fn rejects_noncanonical_stream_identifiers() {
        let stream = test_stream();
        let mut payload = serde_json::to_value(&stream).unwrap();
        payload["stream_id"] = serde_json::json!(stream.stream_id.to_string().to_uppercase());
        assert!(serde_json::from_value::<StreamDescriptor>(payload).is_err());
    }
}
