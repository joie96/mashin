use std::path::PathBuf;
use std::sync::Arc;

pub(crate) enum SourceReadResult {
    Data(Arc<[u8]>),
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

    pub(crate) fn content_key(&self) -> Option<&str> {
        match self {
            Self::CachedContent { content_key } => Some(content_key),
            Self::File { .. } => None,
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
