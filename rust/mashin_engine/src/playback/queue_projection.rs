//! Read-only MA queue snapshots. Requests are invalidated by queue events and target changes.

use crate::domain::identifiers::QueueId;
use crate::playback::models::Queue;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct QueueSnapshotRequest {
    generation: u64,
    revision: u64,
}

pub(crate) struct QueueProjection {
    queue: Queue,
    generation: u64,
    revision: u64,
    is_synchronized: bool,
}

impl QueueProjection {
    pub(crate) fn new(queue_id: QueueId) -> Self {
        Self {
            queue: Queue {
                queue_id,
                ..Queue::default()
            },
            generation: 0,
            revision: 0,
            is_synchronized: false,
        }
    }

    pub(crate) fn select_queue(&mut self, queue_id: QueueId) {
        self.generation += 1;
        self.revision = 0;
        self.is_synchronized = false;
        self.queue = Queue {
            queue_id,
            ..Queue::default()
        };
    }

    pub(crate) fn invalidate(&mut self) {
        self.revision += 1;
        self.is_synchronized = false;
    }

    pub(crate) fn begin_snapshot(&self) -> QueueSnapshotRequest {
        QueueSnapshotRequest {
            generation: self.generation,
            revision: self.revision,
        }
    }

    pub(crate) fn apply_snapshot(&mut self, request: QueueSnapshotRequest, queue: Queue) -> bool {
        if request != self.begin_snapshot() || queue.queue_id != self.queue.queue_id {
            return false;
        }
        self.queue = queue;
        self.is_synchronized = true;
        true
    }

    pub(crate) fn queue(&self) -> &Queue {
        &self.queue
    }

    pub(crate) fn is_synchronized(&self) -> bool {
        self.is_synchronized
    }
}
