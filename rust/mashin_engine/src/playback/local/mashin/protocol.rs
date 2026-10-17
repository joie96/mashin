//! Strict JSON boundary for Mashin V3; application code only sees typed messages.

use super::models::{AudioProfileId, MAX_INTEGER, OutputState, StreamCursor, StreamDescriptor};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub(crate) const MAX_CONTROL_BYTES: usize = 65536;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProtocolError {
    pub(crate) code: String,
    pub(crate) message: String,
    pub(crate) is_retryable: bool,
    pub(crate) retry_after_millis: Option<u64>,
}

impl ProtocolError {
    pub(crate) fn command_failed() -> Self {
        Self {
            code: "command_failed".into(),
            message: "Playback command could not be applied".into(),
            is_retryable: false,
            retry_after_millis: None,
        }
    }
}

macro_rules! fixed_limits {
    ($($name:ident = $value:literal),* $(,)?) => {
        #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
        #[serde(deny_unknown_fields)]
        pub(crate) struct ProtocolLimits {
            $(pub(crate) $name: u64,)*
            pub(crate) max_server_buffer_metadata_bytes: u64,
            pub(crate) max_server_pending_audio_bytes: u64,
            pub(crate) max_control_message_bytes: u64,
        }

        impl ProtocolLimits {
            pub(crate) fn validate(&self) -> Result<(), String> {
                if $(self.$name != $value ||)*
                    !(1..=8_388_608).contains(&self.max_server_buffer_metadata_bytes)
                    || !(1..=1_048_576).contains(&self.max_server_pending_audio_bytes)
                    || !(1..=MAX_CONTROL_BYTES as u64).contains(&self.max_control_message_bytes) {
                    return Err("incompatible Mashin V3 limits".into());
                }
                Ok(())
            }
        }

        #[cfg(test)]
        impl Default for ProtocolLimits {
            fn default() -> Self {
                Self { $($name: $value,)* max_server_buffer_metadata_bytes: 8_388_608, max_server_pending_audio_bytes: 1_048_576, max_control_message_bytes: MAX_CONTROL_BYTES as u64 }
            }
        }
    };
}

fixed_limits! {
    max_server_buffered_chunks = 10,
    max_audio_chunk_payload_bytes = 262144,
    max_unacknowledged_audio_chunks = 1,
    target_audio_chunk_duration_millis = 1000,
    local_seek_step_millis = 1000,
    opus_preroll_duration_millis = 80,
    max_devices = 1,
    max_control_connections = 1,
    max_audio_connections = 1,
    max_active_source_streams = 1,
    max_prepared_next_items = 1,
    max_client_decoders = 1,
    max_retained_streams_per_player = 2,
    max_stream_tombstones_per_player = 32,
    hello_timeout_seconds = 5,
    welcome_timeout_seconds = 30,
    audio_binding_response_timeout_seconds = 5,
    command_ack_timeout_seconds = 5,
    chunk_ack_timeout_seconds = 5,
    audio_setup_timeout_seconds = 30,
    buffer_status_timeout_seconds = 5,
    control_ping_interval_seconds = 5,
    control_liveness_timeout_seconds = 15,
    reconnect_grace_seconds = 30,
    disconnected_source_retention_seconds = 30,
    source_idle_timeout_seconds = 30,
    stream_start_timeout_seconds = 10,
    source_recovery_tail_duration_millis = 3000,
    source_recovery_preroll_duration_millis = 5000,
    source_recovery_search_radius_millis = 1000,
    max_source_recovery_decode_duration_millis = 12000,
    max_source_recovery_attempts_per_incident = 2,
    max_source_recovery_fallbacks_per_incident = 1,
    max_source_recovery_duration_seconds = 30,
    max_source_recovery_attempts_per_player_per_minute = 6,
    shutdown_timeout_seconds = 5,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StreamSnapshot {
    pub(crate) stream: StreamDescriptor,
    pub(crate) next_received_chunk_index: u64,
    pub(crate) next_produced_chunk_index: u64,
    pub(crate) is_complete: bool,
    pub(crate) final_frame_count: Option<u64>,
    pub(crate) error: Option<ProtocolError>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Welcome {
    pub(crate) player_id: String,
    pub(crate) provider_version: String,
    pub(crate) server_revision: String,
    pub(crate) limits: ProtocolLimits,
    pub(crate) audio_profiles: Vec<AudioProfileId>,
    pub(crate) selected_audio_profile: AudioProfileId,
    pub(crate) audio_binding_id: String,
    pub(crate) current_stream: Option<StreamSnapshot>,
    pub(crate) desired_output_state: OutputState,
    pub(crate) media_revision: u64,
    pub(crate) prepared_next: Option<StreamSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    content = "payload",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub(crate) enum ControlCommand {
    SetMedia {
        command_id: u64,
        media_revision: u64,
        #[serde(deserialize_with = "required_stream")]
        current: Option<StreamDescriptor>,
        #[serde(deserialize_with = "required_stream")]
        prepared: Option<StreamDescriptor>,
        initial_output_state: OutputState,
    },
    Play {
        command_id: u64,
        stream_id: Option<Uuid>,
    },
    Pause {
        command_id: u64,
        stream_id: Option<Uuid>,
    },
    Stop {
        command_id: u64,
        stream_id: Option<Uuid>,
    },
    SetVolume {
        command_id: u64,
        volume_percent: u8,
    },
    SetMute {
        command_id: u64,
        is_muted: bool,
    },
    StateRequest {
        command_id: u64,
    },
}

impl ControlCommand {
    pub(crate) fn command_id(&self) -> u64 {
        match self {
            Self::SetMedia { command_id, .. }
            | Self::Play { command_id, .. }
            | Self::Pause { command_id, .. }
            | Self::Stop { command_id, .. }
            | Self::SetVolume { command_id, .. }
            | Self::SetMute { command_id, .. }
            | Self::StateRequest { command_id } => *command_id,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct PreparedActivation {
    pub(crate) previous_stream_id: Uuid,
    pub(crate) stream_id: Uuid,
    pub(crate) media_revision: u64,
    pub(crate) previous_position_frame: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct ResumeState {
    pub(crate) current_stream_id: Option<Uuid>,
    pub(crate) output_revision: u64,
    pub(crate) output_state: OutputState,
    pub(crate) position_frame: u64,
    pub(crate) volume_percent: u8,
    pub(crate) is_muted: bool,
    pub(crate) media_revision: u64,
    pub(crate) received_streams: Vec<StreamCursor>,
    pub(crate) activation: Option<PreparedActivation>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct Capabilities {
    pub(crate) audio_profiles: [AudioProfileId; 5],
    pub(crate) sample_rates_hz: [u32; 2],
    pub(crate) bit_depths: [u8; 1],
    pub(crate) max_channels: u8,
    pub(crate) can_pause: bool,
    pub(crate) can_set_volume: bool,
    pub(crate) can_mute: bool,
}

impl Default for Capabilities {
    fn default() -> Self {
        Self {
            audio_profiles: AudioProfileId::ALL,
            sample_rates_hz: [44100, 48000],
            bit_depths: [16],
            max_channels: 2,
            can_pause: true,
            can_set_volume: true,
            can_mute: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AckStatus {
    Accepted,
    Rejected,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct CommandAck {
    pub(crate) command_id: u64,
    pub(crate) status: AckStatus,
    pub(crate) error: Option<ProtocolError>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
pub(crate) enum ClientMessage {
    PreparedActivated {
        sequence: u64,
        activation: PreparedActivation,
        output_revision: u64,
        output_state: OutputState,
        position_frame: u64,
    },
    Hello {
        device_id: String,
        client_name: String,
        client_version: String,
        resume_state: ResumeState,
        capabilities: Capabilities,
        audio_profile: AudioProfileId,
    },
    AudioHello {
        device_id: String,
        audio_binding_id: String,
        audio_connection_id: u64,
        received_streams: Vec<StreamCursor>,
    },
    CommandAck(CommandAck),
    ChunkAck {
        sequence: u64,
        stream_id: Uuid,
        audio_connection_id: u64,
        chunk_index: u64,
        next_chunk_index: u64,
        status: AckStatus,
        buffer_capacity: usize,
        error: Option<ProtocolError>,
    },
    BufferStatus {
        sequence: u64,
        audio_connection_id: u64,
        buffer_capacity: usize,
    },
    PlayerState {
        sequence: u64,
        stream_id: Option<Uuid>,
        output_revision: u64,
        output_state: OutputState,
        position_frame: u64,
        volume_percent: u8,
        is_muted: bool,
        error: Option<ProtocolError>,
    },
    TrackStarted {
        sequence: u64,
        stream_id: Uuid,
        media_revision: u64,
        output_revision: u64,
        position_frame: u64,
    },
    TrackEnded {
        sequence: u64,
        stream_id: Uuid,
    },
    AudioReconnectRequest {
        sequence: u64,
        request_id: String,
    },
    SetAudioProfile {
        sequence: u64,
        request_id: String,
        audio_profile: AudioProfileId,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(
    tag = "type",
    content = "payload",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub(crate) enum ServerMessage {
    Welcome(Welcome),
    AudioReady {},
    AudioBinding {
        request_id: String,
        audio_binding_id: String,
    },
    AudioProfileChanged {
        request_id: String,
        status: AckStatus,
        selected_audio_profile: AudioProfileId,
        stream_id: Option<Uuid>,
        error: Option<ProtocolError>,
    },
    Error {
        correlation_id: Option<String>,
        stream_id: Option<Uuid>,
        audio_connection_id: Option<u64>,
        error: ProtocolError,
    },
    #[serde(untagged)]
    Command(ControlCommand),
}

pub(crate) struct ServerEnvelope {
    pub(crate) connection_epoch: Option<String>,
    pub(crate) message: ServerMessage,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawEnvelope {
    protocol_version: u8,
    #[serde(rename = "type")]
    kind: String,
    message_id: String,
    #[serde(deserialize_with = "required_epoch")]
    connection_epoch: Option<String>,
    payload: serde_json::Value,
}

fn required_stream<'de, Deserializer: serde::Deserializer<'de>>(
    deserializer: Deserializer,
) -> Result<Option<StreamDescriptor>, Deserializer::Error> {
    Option::<StreamDescriptor>::deserialize(deserializer)
}

fn required_epoch<'de, Deserializer: serde::Deserializer<'de>>(
    deserializer: Deserializer,
) -> Result<Option<String>, Deserializer::Error> {
    Option::<String>::deserialize(deserializer)
}

pub(crate) fn decode(text: &str, expected_epoch: Option<&str>) -> Result<ServerEnvelope, String> {
    if text.len() > MAX_CONTROL_BYTES {
        return Err("Mashin control message is too large".into());
    }
    let raw: RawEnvelope = serde_json::from_str(text).map_err(|_| "invalid Mashin envelope")?;
    if raw.protocol_version != 3
        || raw.message_id.is_empty()
        || !raw.payload.is_object()
        || expected_epoch.is_some_and(|epoch| raw.connection_epoch.as_deref() != Some(epoch))
    {
        return Err("invalid Mashin version, payload, or connection epoch".into());
    }
    let message: ServerMessage =
        serde_json::from_value(serde_json::json!({"type":raw.kind,"payload":raw.payload}))
            .map_err(|_| "invalid Mashin server message")?;
    if let ServerMessage::Command(command) = &message {
        if command.command_id() == 0 || command.command_id() > MAX_INTEGER {
            return Err("invalid Mashin command ID".into());
        }
        if let ControlCommand::SetMedia {
            initial_output_state,
            ..
        } = command
        {
            if !matches!(
                initial_output_state,
                OutputState::Playing | OutputState::Paused
            ) {
                return Err("invalid Mashin initial output state".into());
            }
        }
    }
    Ok(ServerEnvelope {
        connection_epoch: raw.connection_epoch,
        message,
    })
}

pub(crate) fn encode(message: ClientMessage, epoch: Option<&str>) -> Result<String, String> {
    #[derive(Serialize)]
    struct Envelope<'a> {
        protocol_version: u8,
        message_id: String,
        connection_epoch: Option<&'a str>,
        #[serde(flatten)]
        message: ClientMessage,
    }
    let text = serde_json::to_string(&Envelope {
        protocol_version: 3,
        message_id: Uuid::new_v4().to_string(),
        connection_epoch: epoch,
        message,
    })
    .map_err(|_| "encode Mashin message failed")?;
    if text.len() > MAX_CONTROL_BYTES {
        return Err("Mashin control message is too large".into());
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn command(payload: serde_json::Value) -> String {
        serde_json::json!({"protocol_version":3,"type":"set_volume","message_id":"test","connection_epoch":"epoch","payload":payload}).to_string()
    }

    #[test]
    fn rejects_stale_epochs_unknown_fields_and_noninteger_ids() {
        let valid = command(serde_json::json!({"command_id":1,"volume_percent":35}));
        assert!(matches!(
            decode(&valid, Some("epoch")).unwrap().message,
            ServerMessage::Command(ControlCommand::SetVolume {
                volume_percent: 35,
                ..
            })
        ));
        assert!(decode(&valid, Some("old")).is_err());
        assert!(
            decode(
                &command(serde_json::json!({"command_id":true,"volume_percent":35})),
                Some("epoch")
            )
            .is_err()
        );
        assert!(
            decode(
                &command(serde_json::json!({"command_id":1,"volume_percent":35,"extra":0})),
                Some("epoch")
            )
            .is_err()
        );
        let mut missing_epoch: serde_json::Value = serde_json::from_str(&valid).unwrap();
        missing_epoch
            .as_object_mut()
            .unwrap()
            .remove("connection_epoch");
        assert!(decode(&missing_epoch.to_string(), None).is_err());
    }

    #[test]
    fn media_state_is_v3_and_activation_receipt_is_exact() {
        let previous_stream_id = Uuid::new_v4();
        let stream_id = Uuid::new_v4();
        let mut envelope: serde_json::Value = serde_json::from_str(&command(serde_json::json!({
            "command_id": 7, "current": null, "prepared": null,
            "media_revision": 3, "initial_output_state": "paused",
        })))
        .unwrap();
        envelope["type"] = "set_media".into();
        assert!(matches!(
            decode(&envelope.to_string(), Some("epoch"))
                .unwrap()
                .message,
            ServerMessage::Command(ControlCommand::SetMedia {
                media_revision: 3,
                ..
            })
        ));
        for (field, value) in [
            ("position_mode", "preserve"),
            ("desired_output_state", "playing"),
            ("initial_output_state", "idle"),
            ("initial_output_state", "buffering"),
        ] {
            let mut invalid = envelope.clone();
            invalid["payload"][field] = value.into();
            assert!(decode(&invalid.to_string(), Some("epoch")).is_err());
        }
        for version in [1, 2] {
            envelope["protocol_version"] = version.into();
            assert!(decode(&envelope.to_string(), Some("epoch")).is_err());
        }
        envelope["protocol_version"] = 3.into();
        envelope["payload"]
            .as_object_mut()
            .unwrap()
            .remove("prepared");
        assert!(decode(&envelope.to_string(), Some("epoch")).is_err());
        let text = encode(
            ClientMessage::PreparedActivated {
                sequence: 1,
                activation: PreparedActivation {
                    previous_stream_id,
                    stream_id,
                    media_revision: 3,
                    previous_position_frame: 12000,
                },
                output_revision: 2,
                output_state: OutputState::Buffering,
                position_frame: 0,
            },
            Some("epoch"),
        )
        .unwrap();
        let actual: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(actual["protocol_version"], 3);
        assert_eq!(actual["type"], "prepared_activated");
        assert_eq!(
            actual["payload"],
            serde_json::json!({
                "sequence": 1, "activation": {
                    "previous_stream_id": previous_stream_id, "stream_id": stream_id,
                    "media_revision": 3, "previous_position_frame": 12000,
                }, "output_revision": 2, "output_state": "buffering", "position_frame": 0,
            })
        );
    }

    #[test]
    fn outgoing_messages_have_exactly_the_common_envelope() {
        let text = encode(
            ClientMessage::CommandAck(CommandAck {
                command_id: 7,
                status: AckStatus::Accepted,
                error: None,
            }),
            Some("epoch"),
        )
        .unwrap();
        let payload: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(payload.as_object().unwrap().len(), 5);
        assert_eq!(payload["type"], "command_ack");
        assert_eq!(
            payload["payload"],
            serde_json::json!({"command_id":7,"status":"accepted","error":null})
        );
    }
}
