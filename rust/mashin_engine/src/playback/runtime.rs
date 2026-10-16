//! Joined presentation worker owned by the concrete playback engine.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

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
        Self::start(move || {
            if let Some(backend) = backend.upgrade() {
                if let Ok(mut backend) = backend.lock() {
                    backend.process_playback_cycle();
                }
            }
        })
    }

    pub(crate) fn start(run_periodic_work: impl Fn() + Send + 'static) -> Self {
        let should_keep_running = Arc::new(AtomicBool::new(true));
        let should_keep_running_for_thread = Arc::clone(&should_keep_running);
        let task = std::thread::spawn(move || {
            while should_keep_running_for_thread.load(Ordering::Relaxed) {
                run_periodic_work();
                std::thread::park_timeout(Duration::from_millis(25));
            }
        });
        Self {
            should_keep_running,
            task: Some(task),
        }
    }

    // Stops and joins the worker before its native dependencies can be released.
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
    use super::*;

    struct WorkerLifetime(Arc<AtomicBool>);
    impl Drop for WorkerLifetime {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Release);
        }
    }

    #[test]
    fn stop_waits_until_the_worker_releases_its_dependencies() {
        let is_released = Arc::new(AtomicBool::new(false));
        let lifetime = WorkerLifetime(is_released.clone());
        let (sender, receiver) = std::sync::mpsc::channel();
        let mut worker = PeriodicWorker::start(move || {
            std::hint::black_box(&lifetime);
            let _ = sender.send(());
        });
        receiver.recv_timeout(Duration::from_secs(1)).unwrap();
        worker.stop();
        assert!(is_released.load(Ordering::Acquire));
        worker.stop();
    }
}
