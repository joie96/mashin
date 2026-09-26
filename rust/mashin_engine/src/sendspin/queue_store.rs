//! Persistent Sendspin queue snapshots, separate from downloader and playback state.

use crate::music_assistant::models::queue::QueueItem;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Sender};
use std::thread::{self, JoinHandle};

use crate::logging::Logger;

static LOGGER: Logger = Logger::new(module_path!());
pub(crate) const SNAPSHOT_VERSION: u32 = 1;

pub(crate) struct QueueStore {
    sender: Sender<QueueSnapshot>,
    writer: Option<JoinHandle<()>>,
}

impl QueueStore {
    pub(crate) fn start(path: PathBuf) -> Result<(Self, Option<QueueSnapshot>), String> {
        let snapshot = match QueueSnapshot::load(&path) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                LOGGER.error(error);
                None
            }
        };
        let (sender, receiver) = mpsc::channel::<QueueSnapshot>();
        let writer = thread::Builder::new()
            .name("sendspin-queue-store".to_string())
            .spawn(move || {
                while let Ok(mut snapshot) = receiver.recv() {
                    while let Ok(newer) = receiver.try_recv() {
                        snapshot = newer;
                    }
                    if let Err(error) = snapshot.save(&path) {
                        LOGGER.error(error);
                    }
                }
            })
            .map_err(|error| format!("start queue snapshot writer failed: {error}"))?;
        Ok((
            Self {
                sender,
                writer: Some(writer),
            },
            snapshot,
        ))
    }

    pub(crate) fn save(&self, snapshot: QueueSnapshot) {
        if self.sender.send(snapshot).is_err() {
            LOGGER.error("queue snapshot writer has stopped");
        }
    }

    // Finish queued writes before reconfiguration or native shutdown.
    pub(crate) fn finish(mut self) {
        drop(self.sender);
        if let Some(writer) = self.writer.take() {
            if writer.join().is_err() {
                LOGGER.error("queue snapshot writer panicked");
            }
        }
    }
}

pub(crate) fn queue_snapshot_path() -> Result<PathBuf, String> {
    #[cfg(test)]
    {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        thread::current().name().hash(&mut hasher);
        return Ok(std::env::temp_dir().join(format!(
            "mashin-queue-test-{}-{}.json",
            std::process::id(),
            hasher.finish()
        )));
    }
    #[cfg(target_os = "android")]
    {
        return crate::jni::android_context::application_files_directory()
            .map(|path| path.join("sendspin-queue.json"));
    }
    #[cfg(target_os = "windows")]
    {
        return std::env::var_os("LOCALAPPDATA")
            .map(|path| {
                PathBuf::from(path)
                    .join("Mashin")
                    .join("sendspin-queue.json")
            })
            .ok_or_else(|| "LOCALAPPDATA is not set".to_string());
    }
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    {
        return std::env::var_os("HOME")
            .map(|path| {
                PathBuf::from(path).join("Library/Application Support/Mashin/sendspin-queue.json")
            })
            .ok_or_else(|| "HOME is not set".to_string());
    }
    #[cfg(target_os = "linux")]
    {
        let data_directory = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME").map(|path| PathBuf::from(path).join(".local/share"))
            })
            .ok_or_else(|| "neither XDG_DATA_HOME nor HOME is set".to_string())?;
        return Ok(data_directory.join("mashin/sendspin-queue.json"));
    }
    #[allow(unreachable_code)]
    Err("queue snapshots are not supported on this platform".to_string())
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct QueueSnapshot {
    pub(crate) version: u32,
    pub(crate) music_assistant_url: String,
    pub(crate) username: String,
    pub(crate) items: Vec<QueueItem>,
    pub(crate) current_index: Option<usize>,
}

impl QueueSnapshot {
    pub(crate) fn load(path: &Path) -> Result<Option<Self>, String> {
        let contents = match fs::read(path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(format!("read queue snapshot failed: {error}")),
        };
        let snapshot: Self = serde_json::from_slice(&contents)
            .map_err(|error| format!("decode queue snapshot failed: {error}"))?;
        if snapshot.version != SNAPSHOT_VERSION {
            return Err(format!(
                "unsupported queue snapshot version {}",
                snapshot.version
            ));
        }
        Ok(Some(snapshot))
    }

    pub(crate) fn save(&self, path: &Path) -> Result<(), String> {
        let contents = serde_json::to_vec(self)
            .map_err(|error| format!("encode queue snapshot failed: {error}"))?;
        let parent = path
            .parent()
            .ok_or("queue snapshot has no parent directory")?;
        fs::create_dir_all(parent)
            .map_err(|error| format!("create queue snapshot directory failed: {error}"))?;
        let temporary_path = path.with_extension("json.tmp");
        fs::write(&temporary_path, contents)
            .map_err(|error| format!("write queue snapshot failed: {error}"))?;
        fs::rename(&temporary_path, path)
            .map_err(|error| format!("replace queue snapshot failed: {error}"))
    }
}

#[cfg(test)]
mod tests {
    use super::QueueSnapshot;

    #[test]
    fn snapshot_round_trips_and_replaces_saved_value() {
        let path = std::env::temp_dir().join(format!(
            "mashin-queue-snapshot-test-{}-{}.json",
            std::process::id(),
            rand::random::<u64>()
        ));
        let snapshot = QueueSnapshot {
            version: super::SNAPSHOT_VERSION,
            music_assistant_url: "https://example.test".into(),
            username: "listener".into(),
            items: Vec::new(),
            current_index: None,
        };
        snapshot.save(&path).expect("save queue snapshot");
        assert_eq!(
            QueueSnapshot::load(&path).expect("load snapshot"),
            Some(snapshot)
        );
        let empty = QueueSnapshot {
            version: super::SNAPSHOT_VERSION,
            music_assistant_url: "https://example.test".into(),
            username: "another-listener".into(),
            items: Vec::new(),
            current_index: None,
        };
        empty.save(&path).expect("replace snapshot");
        assert_eq!(QueueSnapshot::load(&path).expect("load empty"), Some(empty));
        std::fs::remove_file(path).expect("remove test snapshot");
    }

    #[test]
    fn invalid_snapshot_does_not_prevent_subsequent_saves() {
        let path = super::queue_snapshot_path().expect("test queue path");
        std::fs::write(&path, b"not a queue snapshot").expect("write corrupt snapshot");
        let (store, restored) = super::QueueStore::start(path.clone()).expect("start queue store");
        assert!(restored.is_none());
        let snapshot = QueueSnapshot {
            version: super::SNAPSHOT_VERSION,
            music_assistant_url: "https://example.test".into(),
            username: "listener".into(),
            items: Vec::new(),
            current_index: None,
        };
        store.save(snapshot);
        store.finish();
        assert!(QueueSnapshot::load(&path)
            .expect("load replacement")
            .is_some());
        std::fs::remove_file(path).expect("remove test snapshot");
    }
}
