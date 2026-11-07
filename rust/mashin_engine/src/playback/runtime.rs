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
                std::thread::park_timeout(Duration::from_millis(25));
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
