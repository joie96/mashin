//! Protected credential capability; no configuration or session policy is stored here.

pub(crate) trait CredentialStore: Send {
    fn read(&self) -> Result<Option<String>, String>;
    fn write(&self, serialized: &str) -> Result<(), String>;
    fn clear(&self) -> Result<(), String>;
}

#[cfg(not(target_os = "android"))]
pub(crate) struct KeyringCredentialStore;

#[cfg(not(target_os = "android"))]
impl KeyringCredentialStore {
    // The existing service/account pair remains the protected storage identity.
    fn entry(&self) -> Result<keyring::Entry, String> {
        keyring::Entry::new("Mashin", "music-assistant-session")
            .map_err(|error| format!("create secure session credential failed: {error}"))
    }
}

#[cfg(not(target_os = "android"))]
impl CredentialStore for KeyringCredentialStore {
    // Protected credential I/O.
    fn read(&self) -> Result<Option<String>, String> {
        match self.entry()?.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(format!("read secure credentials failed: {error}")),
        }
    }

    fn write(&self, serialized: &str) -> Result<(), String> {
        self.entry()?
            .set_password(serialized)
            .map_err(|error| format!("save secure credentials failed: {error}"))
    }

    fn clear(&self) -> Result<(), String> {
        match self.entry()?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(format!("clear secure credentials failed: {error}")),
        }
    }
}
