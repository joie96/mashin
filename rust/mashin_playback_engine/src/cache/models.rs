use std::path::PathBuf;

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct CacheContentKey(pub(crate) String);

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CacheEntryState {
    Partial,
    Complete,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(crate) struct CacheEntry {
    pub(crate) key: CacheContentKey,
    pub(crate) path: Option<PathBuf>,
    pub(crate) byte_len: u64,
    pub(crate) duration_millis: u64,
    pub(crate) state: CacheEntryState,
}
