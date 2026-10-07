use std::path::PathBuf;
use std::sync::Arc;

pub(crate) enum SourceReadResult {
    Data {
        data: Arc<[u8]>,
        frame_count: u64,
        leading_frames_to_skip: u64,
    },
    Waiting,
    EndOfStream,
    Error(String),
}

#[allow(dead_code)]
pub(crate) enum AudioSource {
    CachedContent { content_key: String },
    File { path: PathBuf },
}

impl AudioSource {
    pub(crate) fn cached_content(content_key: impl Into<String>) -> Self {
        Self::CachedContent {
            content_key: content_key.into(),
        }
    }

    #[allow(dead_code)]
    pub(crate) fn file(path: impl Into<PathBuf>) -> Self {
        Self::File { path: path.into() }
    }

    #[allow(dead_code)]
    pub(crate) fn file_path(&self) -> Option<&PathBuf> {
        match self {
            Self::CachedContent { .. } => None,
            Self::File { path } => Some(path),
        }
    }
}
