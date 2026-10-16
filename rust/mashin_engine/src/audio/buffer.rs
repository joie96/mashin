//! Immutable encoded storage retains the original allocation and its payload range.

use super::models::{AudioStream, ChunkCursor};
use std::collections::VecDeque;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub(crate) struct EncodedChunk {
    pub(crate) stream_id: Uuid,
    pub(crate) index: u64,
    pub(crate) first_frame: u64,
    pub(crate) frame_count: u64,
    pub(crate) is_final: bool,
    pub(crate) encoded_bytes: Arc<[u8]>,
    pub(crate) payload_offset: usize,
}

impl EncodedChunk {
    // The retained allocation includes framing bytes, but decoders see only the payload.
    pub(crate) fn payload(&self) -> &[u8] {
        &self.encoded_bytes[self.payload_offset..]
    }
}

pub(crate) struct CachedStream {
    pub(crate) descriptor: AudioStream,
    chunks: VecDeque<EncodedChunk>,
    next_chunk_index: u64,
    retained_bytes: usize,
    next_frame: u64,
    is_complete: bool,
}

impl CachedStream {
    fn new(descriptor: AudioStream) -> Result<Self, String> {
        descriptor.validate()?;
        Ok(Self {
            descriptor,
            chunks: VecDeque::new(),
            next_chunk_index: 1,
            retained_bytes: 0,
            next_frame: 0,
            is_complete: false,
        })
    }

    pub(crate) fn cursor(&self) -> ChunkCursor {
        ChunkCursor {
            stream_id: self.descriptor.stream_id,
            next_chunk_index: self.next_chunk_index,
        }
    }

    pub(crate) fn chunk(&self, index: u64) -> Option<&EncodedChunk> {
        let offset = index.checked_sub(self.chunks.front()?.index)?;
        self.chunks.get(usize::try_from(offset).ok()?)
    }
    pub(crate) fn is_complete(&self) -> bool {
        self.is_complete
    }
    pub(crate) fn received_frames(&self) -> u64 {
        self.next_frame
    }

    fn retain(&mut self, chunk: EncodedChunk) -> Result<(), String> {
        let sample_rate = self.descriptor.output_format.sample_rate_hz as u64;
        if self.is_complete
            || chunk.frame_count == 0
            || chunk.payload_offset >= chunk.encoded_bytes.len()
            || chunk.index != self.cursor().next_chunk_index
            || chunk.first_frame != self.next_frame
            || chunk.frame_count > sample_rate
            || (!chunk.is_final && chunk.frame_count != sample_rate)
        {
            return Err("audio chunks must be contiguous one-second frames".into());
        }
        let next_frame = self
            .next_frame
            .checked_add(chunk.frame_count)
            .ok_or("audio frame cursor exhausted")?;
        let next_chunk_index = self
            .next_chunk_index
            .checked_add(1)
            .ok_or("audio chunk cursor exhausted")?;
        self.next_frame = next_frame;
        self.next_chunk_index = next_chunk_index;
        self.retained_bytes += chunk.encoded_bytes.len();
        self.is_complete = chunk.is_final;
        self.chunks.push_back(chunk);
        Ok(())
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct BufferStatistics {
    pub(crate) retained_bytes: usize,
    pub(crate) byte_capacity: usize,
    pub(crate) cached_chunks: usize,
    pub(crate) current_remaining_millis: u64,
    pub(crate) prepared_remaining_millis: u64,
}

pub(crate) struct StreamBuffer {
    current: Option<CachedStream>,
    prepared: Option<CachedStream>,
    next_revision: u64,
    byte_capacity: usize,
}

impl StreamBuffer {
    // Configuration and authorized ownership changes.
    pub(crate) fn new(byte_capacity: usize) -> Self {
        Self {
            current: None,
            prepared: None,
            next_revision: 0,
            byte_capacity,
        }
    }

    pub(crate) fn set_current(&mut self, descriptor: AudioStream) -> Result<(), String> {
        let stream = CachedStream::new(descriptor)?;
        self.current = Some(stream);
        self.prepared = None;
        self.next_revision = 0;
        Ok(())
    }

    pub(crate) fn prepare_next(
        &mut self,
        current_id: Uuid,
        descriptor: AudioStream,
        revision: u64,
    ) -> Result<(), String> {
        if self
            .current
            .as_ref()
            .map(|current| current.descriptor.stream_id)
            != Some(current_id)
            || self.prepared.is_some()
            || descriptor.stream_id == current_id
            || revision <= self.next_revision
        {
            return Err("stale audio successor authorization".into());
        }
        let stream = CachedStream::new(descriptor)?;
        self.prepared = Some(stream);
        self.next_revision = revision;
        Ok(())
    }

    pub(crate) fn clear_next(&mut self, current_id: Uuid, revision: u64) -> Result<(), String> {
        if self
            .current
            .as_ref()
            .map(|current| current.descriptor.stream_id)
            != Some(current_id)
            || revision <= self.next_revision
        {
            return Err("stale audio successor revocation".into());
        }
        self.prepared = None;
        self.next_revision = revision;
        Ok(())
    }

    pub(crate) fn promote_next(
        &mut self,
        audible_position_frame: u64,
    ) -> Result<(Uuid, u64), String> {
        if !self.current.as_ref().is_some_and(|current| {
            current.is_complete && audible_position_frame >= current.next_frame
        }) {
            return Err("audio current stream has not audibly ended".into());
        }
        let next = self
            .prepared
            .take()
            .ok_or("no authorized audio successor")?;
        if next.chunks.is_empty() {
            self.prepared = Some(next);
            return Err("audio successor is not buffered".into());
        }
        let stream_id = next.descriptor.stream_id;
        self.current = Some(next);
        Ok((stream_id, self.next_revision))
    }

    // Explicit activation consumes only the exact previously authorized successor.
    pub(crate) fn activate_prepared(
        &mut self,
        current_stream_id: Uuid,
        stream_id: Uuid,
        next_revision: u64,
    ) -> Result<(), String> {
        if self
            .current
            .as_ref()
            .map(|stream| stream.descriptor.stream_id)
            != Some(current_stream_id)
            || self
                .prepared
                .as_ref()
                .map(|stream| stream.descriptor.stream_id)
                != Some(stream_id)
            || self.next_revision != next_revision
        {
            return Err("stale prepared activation".into());
        }
        self.current = self.prepared.take();
        Ok(())
    }

    pub(crate) fn clear(&mut self) {
        self.current = None;
        self.prepared = None;
        self.next_revision = 0;
    }

    pub(crate) fn release_played_chunks(&mut self, position_frame: u64) {
        if let Some(current) = self.current.as_mut() {
            while current
                .chunks
                .front()
                .is_some_and(|chunk| chunk.first_frame + chunk.frame_count <= position_frame)
            {
                let chunk = current.chunks.pop_front().unwrap();
                current.retained_bytes -= chunk.encoded_bytes.len();
            }
        }
    }

    // Receipt is acknowledged only after the original bytes enter the bounded cache.
    pub(crate) fn retain(&mut self, chunk: EncodedChunk) -> Result<(), String> {
        if chunk.encoded_bytes.len() > self.byte_capacity.saturating_sub(self.retained_bytes()) {
            return Err("audio encoded cache is full".into());
        }
        if let Some(current) = self
            .current
            .as_mut()
            .filter(|stream| stream.descriptor.stream_id == chunk.stream_id)
        {
            return current.retain(chunk);
        }
        if let Some(prepared) = self
            .prepared
            .as_mut()
            .filter(|stream| stream.descriptor.stream_id == chunk.stream_id)
        {
            return prepared.retain(chunk);
        }
        Err("audio chunk belongs to an unauthorized stream".into())
    }

    // Queries never expose streaming-provider media URLs.
    pub(crate) fn statistics(&self, position_frame: u64) -> BufferStatistics {
        let remaining_millis = |stream: &CachedStream, position: u64| {
            stream.received_frames().saturating_sub(position) * 1000
                / u64::from(stream.descriptor.output_format.sample_rate_hz)
        };
        BufferStatistics {
            retained_bytes: self.retained_bytes(),
            byte_capacity: self.byte_capacity,
            cached_chunks: self
                .current
                .iter()
                .chain(self.prepared.iter())
                .map(|stream| stream.chunks.len())
                .sum(),
            current_remaining_millis: self
                .current
                .as_ref()
                .map_or(0, |stream| remaining_millis(stream, position_frame)),
            prepared_remaining_millis: self
                .prepared
                .as_ref()
                .map_or(0, |stream| remaining_millis(stream, 0)),
        }
    }

    pub(crate) fn current(&self) -> Option<&CachedStream> {
        self.current.as_ref()
    }
    pub(crate) fn prepared(&self) -> Option<&CachedStream> {
        self.prepared.as_ref()
    }
    pub(crate) fn next_revision(&self) -> u64 {
        self.next_revision
    }
    pub(crate) fn retained_bytes(&self) -> usize {
        self.current
            .as_ref()
            .map_or(0, |stream| stream.retained_bytes)
            + self
                .prepared
                .as_ref()
                .map_or(0, |stream| stream.retained_bytes)
    }
    pub(crate) fn advertised_capacity(&self) -> usize {
        self.byte_capacity.saturating_sub(self.retained_bytes())
    }
    pub(crate) fn cursors(&self) -> Vec<ChunkCursor> {
        self.current
            .iter()
            .chain(self.prepared.iter())
            .map(CachedStream::cursor)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::models::test_stream;

    fn chunk(
        stream_id: Uuid,
        index: u64,
        first: u64,
        frames: u64,
        final_chunk: bool,
    ) -> EncodedChunk {
        let mut bytes = vec![0; 41];
        bytes.extend_from_slice(&[1, 2, 3]);
        EncodedChunk {
            stream_id,
            index,
            first_frame: first,
            frame_count: frames,
            is_final: final_chunk,
            encoded_bytes: bytes.into(),
            payload_offset: 41,
        }
    }

    #[test]
    fn statistics_separate_current_audible_ahead_from_prepared_audio() {
        let mut cache = StreamBuffer::new(1000);
        let current = test_stream();
        let next = test_stream();
        cache.set_current(current.clone()).unwrap();
        cache
            .prepare_next(current.stream_id, next.clone(), 1)
            .unwrap();
        cache
            .retain(chunk(current.stream_id, 1, 0, 48000, false))
            .unwrap();
        cache
            .retain(chunk(next.stream_id, 1, 0, 48000, false))
            .unwrap();
        assert_eq!(
            cache.statistics(24000),
            BufferStatistics {
                retained_bytes: 88,
                byte_capacity: 1000,
                cached_chunks: 2,
                current_remaining_millis: 500,
                prepared_remaining_millis: 1000,
            }
        );
        cache.release_played_chunks(48000);
        assert_eq!(
            cache.statistics(96000),
            BufferStatistics {
                retained_bytes: 44,
                byte_capacity: 1000,
                cached_chunks: 1,
                current_remaining_millis: 0,
                prepared_remaining_millis: 1000,
            }
        );
        cache.clear();
        assert_eq!(
            cache.statistics(96000),
            BufferStatistics {
                retained_bytes: 0,
                byte_capacity: 1000,
                cached_chunks: 0,
                current_remaining_millis: 0,
                prepared_remaining_millis: 0,
            }
        );
    }

    #[test]
    fn retains_only_authorized_a_and_b_and_counts_headers() {
        let mut cache = StreamBuffer::new(1000);
        let current = test_stream();
        let next = test_stream();
        cache.set_current(current.clone()).unwrap();
        cache
            .prepare_next(current.stream_id, next.clone(), 1)
            .unwrap();
        cache
            .retain(chunk(current.stream_id, 1, 0, 48000, false))
            .unwrap();
        cache
            .retain(chunk(next.stream_id, 1, 0, 48000, false))
            .unwrap();
        assert_eq!(cache.retained_bytes(), 88);
        assert_eq!(cache.advertised_capacity(), 912);
        assert!(
            cache
                .retain(chunk(Uuid::new_v4(), 1, 0, 48000, false))
                .unwrap_err()
                .contains("unauthorized")
        );
        assert!(
            cache
                .prepare_next(current.stream_id, test_stream(), 2)
                .is_err()
        );
        assert_eq!(cache.cursors().len(), 2);
    }

    #[test]
    fn rejected_receipt_never_advances_cursor() {
        let mut cache = StreamBuffer::new(44);
        let current = test_stream();
        cache.set_current(current.clone()).unwrap();
        assert!(
            cache
                .retain(chunk(current.stream_id, 2, 48000, 48000, false))
                .is_err()
        );
        assert_eq!(cache.cursors()[0].next_chunk_index, 1);
        cache
            .retain(chunk(current.stream_id, 1, 0, 48000, false))
            .unwrap();
        assert!(
            cache
                .retain(chunk(current.stream_id, 2, 48000, 48000, false))
                .is_err()
        );
        assert_eq!(cache.cursors()[0].next_chunk_index, 2);
    }

    #[test]
    fn revoking_b_frees_capacity_and_rejects_its_chunks() {
        let mut cache = StreamBuffer::new(1000);
        let current = test_stream();
        let next = test_stream();
        cache.set_current(current.clone()).unwrap();
        cache
            .prepare_next(current.stream_id, next.clone(), 1)
            .unwrap();
        cache
            .retain(chunk(next.stream_id, 1, 0, 48000, false))
            .unwrap();
        cache.clear_next(current.stream_id, 2).unwrap();
        assert_eq!(cache.retained_bytes(), 0);
        assert!(
            cache
                .retain(chunk(next.stream_id, 2, 48000, 48000, false))
                .is_err()
        );
        assert!(
            cache
                .prepare_next(current.stream_id, test_stream(), 1)
                .is_err()
        );
    }

    #[test]
    fn promotion_discards_a_but_keeps_original_b_bytes() {
        let mut cache = StreamBuffer::new(1000);
        let current = test_stream();
        let next = test_stream();
        cache.set_current(current.clone()).unwrap();
        cache
            .prepare_next(current.stream_id, next.clone(), 7)
            .unwrap();
        cache
            .retain(chunk(current.stream_id, 1, 0, 123, true))
            .unwrap();
        cache
            .retain(chunk(next.stream_id, 1, 0, 48000, false))
            .unwrap();
        assert!(cache.promote_next(122).is_err());
        assert_eq!(cache.promote_next(123).unwrap(), (next.stream_id, 7));
        assert_eq!(cache.retained_bytes(), 44);
        assert_eq!(
            cache.current().unwrap().chunk(1).unwrap().payload(),
            &[1, 2, 3]
        );
        assert!(cache.prepared().is_none());
    }

    #[test]
    fn retention_rejects_invalid_chunk_frame_counts() {
        let mut cache = StreamBuffer::new(1000);
        let current = test_stream();
        cache.set_current(current.clone()).unwrap();
        assert!(
            cache
                .retain(chunk(current.stream_id, 1, 0, 48001, true))
                .is_err()
        );
        assert!(
            cache
                .retain(chunk(current.stream_id, 1, 0, 100, false))
                .is_err()
        );
        cache
            .retain(chunk(current.stream_id, 1, 0, 100, true))
            .unwrap();
        assert!(cache.current().unwrap().is_complete());
    }

    #[test]
    fn played_chunks_free_capacity_without_rewinding_receipt_cursor() {
        let mut cache = StreamBuffer::new(88);
        let current = test_stream();
        cache.set_current(current.clone()).unwrap();
        cache
            .retain(chunk(current.stream_id, 1, 0, 48000, false))
            .unwrap();
        cache
            .retain(chunk(current.stream_id, 2, 48000, 48000, false))
            .unwrap();
        cache.release_played_chunks(47999);
        assert_eq!(cache.retained_bytes(), 88);
        cache.release_played_chunks(48000);
        assert_eq!(cache.retained_bytes(), 44);
        assert_eq!(cache.cursors()[0].next_chunk_index, 3);
        assert!(cache.current().unwrap().chunk(1).is_none());
        assert_eq!(cache.current().unwrap().chunk(2).unwrap().index, 2);
        cache
            .retain(chunk(current.stream_id, 3, 96000, 48000, false))
            .unwrap();
        assert_eq!(cache.cursors()[0].next_chunk_index, 4);
    }
}
