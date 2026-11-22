//! Joined presentation worker owned by the concrete playback engine.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

const PRESENTATION_INTERVAL: Duration = Duration::from_millis(250);

pub(crate) struct PeriodicWorker {
    should_keep_running: Arc<AtomicBool>,
    task: Option<JoinHandle<()>>,
}

impl PeriodicWorker {
    // Worker lifecycle.
    pub(crate) fn for_backend<Backend: super::backend::PlaybackBackend + 'static>(
        backend: &Arc<Mutex<Backend>>,
    ) -> Self {
        let backend = Arc::downgrade(backend);
        Self::start(move |should_keep_running| {
            let Some(backend) = backend.upgrade() else {
                return;
            };
            let events = match backend.lock() {
                Ok(mut backend) => backend.collect_presentation_events(),
                Err(_) => return,
            };
            events.publish(|| should_keep_running.load(Ordering::Acquire));
        })
    }

    pub(crate) fn start(run_periodic_work: impl Fn(&AtomicBool) + Send + 'static) -> Self {
        let should_keep_running = Arc::new(AtomicBool::new(true));
        let should_keep_running_for_thread = Arc::clone(&should_keep_running);
        let task = std::thread::spawn(move || {
            while should_keep_running_for_thread.load(Ordering::Relaxed) {
                run_periodic_work(&should_keep_running_for_thread);
                std::thread::park_timeout(PRESENTATION_INTERVAL);
            }
        });
        Self {
            should_keep_running,
            task: Some(task),
        }
    }

    // Request cancellation under state locks; the caller joins completion outside them.
    pub(crate) fn shutdown(
        mut self,
        runtime: &tokio::runtime::Handle,
    ) -> tokio::task::JoinHandle<()> {
        self.should_keep_running.store(false, Ordering::Relaxed);
        let task = self.task.take();
        if let Some(task) = &task {
            task.thread().unpark();
        }
        runtime.spawn_blocking(move || {
            if let Some(task) = task {
                if let Err(panic) = task.join() {
                    std::panic::resume_unwind(panic);
                }
            }
        })
    }

    // Fallback for an owner dropped without transferring its explicit completion handle.
    pub(crate) fn stop(&mut self) {
        self.should_keep_running.store(false, Ordering::Relaxed);
        if let Some(task) = self.task.take() {
            task.thread().unpark();
            task.join().expect("playback presentation worker panicked");
        }
    }
}

impl Drop for PeriodicWorker {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::{PeriodicWorker, PRESENTATION_INTERVAL};
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    // Presentation cadence is independent of audio callbacks and remains interruptible on shutdown.
    #[test]
    fn presentation_ticks_are_spaced_by_250_milliseconds() {
        let (sender, receiver) = mpsc::channel();
        let mut worker = PeriodicWorker::start(move |_| {
            sender.send(Instant::now()).expect("tick receiver is alive");
        });
        let first = receiver.recv_timeout(Duration::from_secs(2)).unwrap();
        let second = receiver.recv_timeout(Duration::from_secs(2)).unwrap();
        worker.stop();
        assert!(second.duration_since(first) >= PRESENTATION_INTERVAL);
        assert_eq!(PRESENTATION_INTERVAL, Duration::from_millis(250));
    }

    #[test]
    fn shutdown_wakes_the_parked_presentation_worker() {
        let (sender, receiver) = mpsc::channel();
        let worker = PeriodicWorker::start(move |_| {
            sender.send(()).expect("tick receiver is alive");
        });
        receiver.recv_timeout(Duration::from_secs(2)).unwrap();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let completion = worker.shutdown(runtime.handle());
        runtime.block_on(async {
            tokio::time::timeout(Duration::from_secs(2), completion)
                .await
                .unwrap()
                .unwrap();
        });
        assert!(receiver.try_recv().is_err());
    }
}
