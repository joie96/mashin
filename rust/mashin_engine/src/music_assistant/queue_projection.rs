//! Read-only MA queue snapshots. Requests are invalidated by queue events and target changes.

use crate::music_assistant::models::identifiers::QueueId;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn queue(queue_id: &str) -> Queue {
        Queue {
            queue_id: QueueId(queue_id.into()),
            ..Queue::default()
        }
    }

    #[test]
    fn accepts_only_current_target_snapshot() {
        let mut projection = QueueProjection::new(QueueId("first".into()));
        let previous = projection.begin_snapshot();
        projection.select_queue(QueueId("second".into()));
        assert!(!projection.apply_snapshot(previous, queue("first")));
        assert!(projection.apply_snapshot(projection.begin_snapshot(), queue("second")));
        assert!(projection.is_synchronized());
    }

    #[test]
    fn events_during_snapshot_require_a_new_snapshot() {
        let mut projection = QueueProjection::new(QueueId("player".into()));
        let request = projection.begin_snapshot();
        projection.invalidate();
        assert!(!projection.apply_snapshot(request, queue("player")));
        assert!(!projection.is_synchronized());
        assert!(projection.apply_snapshot(projection.begin_snapshot(), queue("player")));
    }

    #[test]
    fn disconnection_keeps_last_known_queue_but_marks_it_stale() {
        let mut projection = QueueProjection::new(QueueId("player".into()));
        assert!(projection.apply_snapshot(projection.begin_snapshot(), queue("player")));
        projection.invalidate();
        assert_eq!(projection.queue().queue_id.0, "player");
        assert!(!projection.is_synchronized());
    }
}
