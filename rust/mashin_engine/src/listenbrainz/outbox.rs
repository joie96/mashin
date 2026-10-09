//! Token-free, account-scoped listen persistence; only the submission worker owns this outbox.

use super::client::Listen;
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct PendingListen {
    pub(super) username: String,
    pub(super) listen: Listen,
}

pub(super) struct ListenOutbox {
    path: PathBuf,
    pending: Vec<PendingListen>,
}

impl ListenOutbox {
    // Construction and persistence configuration.
    pub(super) fn load_default() -> Result<Self, String> {
        let path = crate::sendspin::queue_store::queue_snapshot_path()?
            .with_file_name("listenbrainz-pending.json");
        Self::load(path)
    }

    fn load(path: PathBuf) -> Result<Self, String> {
        let pending = match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map_err(|_| "ListenBrainz pending listens could not be decoded".to_string())?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(_) => return Err("ListenBrainz pending listens could not be read".to_string()),
        };
        Ok(Self { path, pending })
    }

    // Outbox commands and durable writes.
    pub(super) fn enqueue(&mut self, listen: PendingListen) {
        self.pending.push(listen);
    }

    pub(super) fn acknowledge(&mut self, username: &str, count: usize) {
        let mut removed = 0;
        self.pending.retain(|item| {
            if item.username == username && removed < count {
                removed += 1;
                false
            } else {
                true
            }
        });
    }

    pub(super) fn persist(&self) -> Result<(), String> {
        write_pending(&self.path, &self.pending)
    }

    // Account-scoped queries.
    pub(super) fn batch(&self, username: &str) -> Vec<Listen> {
        self.pending
            .iter()
            .filter(|item| item.username == username)
            .take(100)
            .map(|item| item.listen.clone())
            .collect()
    }
}

// Flush before replacement so a terminated process cannot leave a partially written JSON document.
fn write_pending(path: &Path, pending: &[PendingListen]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|_| "ListenBrainz outbox directory could not be created".to_string())?;
    }
    let bytes = serde_json::to_vec(pending)
        .map_err(|_| "ListenBrainz pending listens could not be encoded".to_string())?;
    let temporary = path.with_extension("tmp");
    let mut file = std::fs::File::create(&temporary)
        .map_err(|_| "ListenBrainz pending listens could not be written".to_string())?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| "ListenBrainz pending listens could not be flushed".to_string())?;
    drop(file);
    std::fs::rename(temporary, path)
        .map_err(|_| "ListenBrainz pending listens could not be committed".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::listenbrainz::client::test_listen;

    #[test]
    fn outbox_survives_reopening_and_contains_no_token() {
        let path = std::env::temp_dir().join(format!(
            "mashin-listenbrainz-outbox-{}.json",
            std::process::id()
        ));
        let mut outbox = ListenOutbox::load(path.clone()).unwrap();
        outbox.enqueue(PendingListen {
            username: "user".into(),
            listen: test_listen(),
        });
        outbox.persist().unwrap();
        let mut restored = ListenOutbox::load(path.clone()).unwrap();
        assert_eq!(restored.batch("user").len(), 1);
        assert_eq!(restored.batch("user")[0].listened_at, 123);
        assert!(!std::fs::read_to_string(&path).unwrap().contains("token"));
        restored.acknowledge("user", 1);
        restored.persist().unwrap();
        assert!(ListenOutbox::load(path.clone())
            .unwrap()
            .batch("user")
            .is_empty());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn account_changes_never_submit_or_remove_another_users_listens() {
        let mut outbox = ListenOutbox {
            path: PathBuf::new(),
            pending: Vec::new(),
        };
        for username in ["one", "two", "one"] {
            outbox.enqueue(PendingListen {
                username: username.into(),
                listen: test_listen(),
            });
        }
        assert_eq!(outbox.batch("one").len(), 2);
        assert_eq!(outbox.batch("two").len(), 1);
        outbox.acknowledge("one", 1);
        assert_eq!(outbox.batch("one").len(), 1);
        assert_eq!(outbox.batch("two").len(), 1);
    }
}
