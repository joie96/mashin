//! Mashin binary framing is validated before immutable chunks reach native audio.

use super::models::{HEADER_BYTES, MAX_INTEGER, MAX_PAYLOAD_BYTES};
use crate::audio::buffer::EncodedChunk;
use std::sync::Arc;
use uuid::Uuid;

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
