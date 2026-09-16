//! Stoppable periodic worker owned by a local playback engine.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

pub(crate) struct PeriodicWorker {
    should_keep_running: Arc<AtomicBool>,
    task: Option<JoinHandle<()>>,
}

impl PeriodicWorker {
    // Worker lifecycle.
    pub(crate) fn start(run_periodic_work: impl Fn() + Send + 'static) -> Self {
        let should_keep_running = Arc::new(AtomicBool::new(true));
        let should_keep_running_for_thread = Arc::clone(&should_keep_running);
        let task = std::thread::spawn(move || {
            while should_keep_running_for_thread.load(Ordering::Relaxed) {
                run_periodic_work();
                std::thread::sleep(Duration::from_millis(25));
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
        self.task.take();
    }
}

impl Drop for PeriodicWorker {
    fn drop(&mut self) {
        self.stop();
    }
}
