//! The selected control target is independent of the local receiver's lifetime.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PlaybackMode {
    Local,
    Remote,
}

#[cfg(test)]
mod tests {
    use super::PlaybackMode;

    #[test]
    fn modes_have_one_canonical_platform_representation() {
        for (mode, name) in [
            (PlaybackMode::Local, "local"),
            (PlaybackMode::Remote, "remote"),
        ] {
            assert_eq!(serde_json::to_value(mode).unwrap(), name);
            assert_eq!(
                serde_json::from_value::<PlaybackMode>(serde_json::json!(name)).unwrap(),
                mode
            );
        }
        for removed in ["mashin", "sendspin", "offline"] {
            assert!(serde_json::from_value::<PlaybackMode>(serde_json::json!(removed)).is_err());
        }
    }
}
