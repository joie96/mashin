use std::path::{Path, PathBuf};

#[allow(dead_code)]
pub(crate) struct FileAudioSource {
    path: PathBuf,
}

#[allow(dead_code)]
impl FileAudioSource {
    pub(crate) fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}
