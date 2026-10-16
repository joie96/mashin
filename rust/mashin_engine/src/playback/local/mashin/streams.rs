//! Mashin binary framing is validated before immutable chunks reach native audio.

use super::models::{HEADER_BYTES, MAX_INTEGER, MAX_PAYLOAD_BYTES};
use crate::audio::buffer::EncodedChunk;
use std::sync::Arc;
use uuid::Uuid;

pub(crate) fn validate_successor(
    status: &crate::audio::models::AudioStatus,
    current: Uuid,
    revision: u64,
) -> Result<(), String> {
    if status.current_stream_id != Some(current)
        || revision <= status.next_revision
        || revision > MAX_INTEGER
    {
        return Err("stale Mashin successor authorization".into());
    }
    Ok(())
}

pub(crate) fn parse_chunk(wire_bytes: Arc<[u8]>) -> Result<EncodedChunk, String> {
    if wire_bytes.len() <= HEADER_BYTES || wire_bytes.len() > HEADER_BYTES + MAX_PAYLOAD_BYTES {
        return Err("invalid Mashin audio message size".into());
    }
    let stream_id = Uuid::from_slice(&wire_bytes[..16]).map_err(|_| "invalid Mashin stream ID")?;
    let index = u64::from_be_bytes(wire_bytes[16..24].try_into().unwrap());
    let first_frame = u64::from_be_bytes(wire_bytes[24..32].try_into().unwrap());
    let frame_count = u64::from_be_bytes(wire_bytes[32..40].try_into().unwrap());
    let flag = wire_bytes[40];
    if stream_id.get_version_num() != 4
        || stream_id.get_variant() != uuid::Variant::RFC4122
        || index == 0
        || index >= MAX_INTEGER
        || frame_count == 0
        || first_frame > MAX_INTEGER
        || frame_count > MAX_INTEGER - first_frame
        || flag > 1
    {
        return Err("invalid Mashin chunk header".into());
    }
    Ok(EncodedChunk {
        stream_id,
        index,
        first_frame,
        frame_count,
        is_final: flag == 1,
        encoded_bytes: wire_bytes,
        payload_offset: HEADER_BYTES,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message() -> Vec<u8> {
        let mut bytes = Uuid::new_v4().as_bytes().to_vec();
        bytes.extend_from_slice(&1u64.to_be_bytes());
        bytes.extend_from_slice(&0u64.to_be_bytes());
        bytes.extend_from_slice(&480u64.to_be_bytes());
        bytes.extend_from_slice(&[1, 10, 20]);
        bytes
    }

    #[test]
    fn parsed_audio_retains_the_original_allocation_without_decoding_framing() {
        let bytes: Arc<[u8]> = message().into();
        let chunk = parse_chunk(bytes.clone()).unwrap();
        assert!(Arc::ptr_eq(&bytes, &chunk.encoded_bytes));
        assert_eq!(chunk.payload(), &[10, 20]);
        assert_eq!(chunk.first_frame, 0);
        assert_eq!(chunk.frame_count, 480);
        assert_eq!(chunk.index, 1);
        assert!(chunk.is_final);
    }

    #[test]
    fn malformed_wire_chunks_never_cross_the_audio_boundary() {
        assert!(parse_chunk(Arc::from([0u8; HEADER_BYTES])).is_err());
        let mut bytes = message();
        bytes[40] = 2;
        assert!(parse_chunk(bytes.into()).is_err());
        let mut bytes = message();
        bytes[16..24].copy_from_slice(&MAX_INTEGER.to_be_bytes());
        assert!(parse_chunk(bytes.into()).is_err());
        let mut bytes = message();
        bytes.resize(HEADER_BYTES + MAX_PAYLOAD_BYTES + 1, 0);
        assert!(parse_chunk(bytes.into()).is_err());
    }
}
