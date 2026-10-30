//! Mashin's device-local receiver. MA owns authorization; this module owns encoded A/B and DAC state.

pub(crate) mod audio_adapter;
pub(crate) mod models;
pub(crate) mod protocol;
pub(crate) mod receiver;
pub(crate) mod recovery;
pub(crate) mod streams;
