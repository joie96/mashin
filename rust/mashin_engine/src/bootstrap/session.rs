//! Concrete persistence capabilities for application session ownership.

use crate::application::session::SessionStorage;
use crate::configuration::ConfigurationFile;
use crate::credentials::session_store::SplitSessionStorage;
use crate::credentials::store::CredentialStore;
use std::path::PathBuf;

// Session configuration and online capability composition.
pub(crate) fn environment() -> crate::application::session::SessionEnvironment {
    let cache_owner = std::sync::Arc::new(crate::data::cache::catalog::SessionCacheOwner::new(
        application_data_directory,
    ));
    let shutdown_owner = cache_owner.clone();
    crate::application::session::SessionEnvironment {
        authenticate: |login| Box::pin(crate::data::music_assistant::session::authenticate(login)),
        client_name: local_client_name,
        connect: crate::data::music_assistant::connection::MusicAssistantConnection::new,
        app_settings: std::sync::Arc::new(ConfigurationFile::new(configuration_path)),
        repositories: std::sync::Arc::new(move |client, _server, user, generation| {
            let cache_owner = cache_owner.clone();
            Box::pin(async move {
                let catalog = cache_owner.session(client.clone(), user, generation);
                let mut services = online_services(client.clone());
                services.images =
                    std::sync::Arc::new(crate::data::cache::image_loader::SessionImages::new(
                        cache_owner,
                        client,
                        generation,
                    ));
                let catalog = std::sync::Arc::new(catalog);
                services.providers = catalog.clone();
                services.catalog = catalog;
                services.recommendations =
                    std::sync::Arc::new(crate::application::recommendations::Recommendations::new(
                        services.catalog.clone(),
                    ));
                Ok(services)
            })
        }),
        shutdown_repositories: std::sync::Arc::new(move |generation| {
            let owner = shutdown_owner.clone();
            Box::pin(async move { owner.shutdown(generation).await })
        }),
    }
}

pub(crate) fn online_services(
    client: std::sync::Arc<crate::data::music_assistant::client::MusicAssistantClient>,
) -> crate::application::repositories::SessionServices {
    let image_owner =
        std::sync::Arc::new(crate::data::cache::catalog::SessionCacheOwner::new(|| {
            Err("persistent cache unavailable in online-only services".into())
        }));
    crate::application::repositories::SessionServices {
        images: std::sync::Arc::new(crate::data::cache::image_loader::SessionImages::new(
            image_owner,
            client.clone(),
            0,
        )),
        recommendations: std::sync::Arc::new(
            crate::application::recommendations::Recommendations::new(client.clone()),
        ),
        catalog: client.clone(),
        providers: client.clone(),
        library: client,
    }
}

// Select the platform capability without storage importing a native adapter.
pub(crate) fn create_session_storage() -> Box<dyn SessionStorage> {
    #[cfg(target_os = "android")]
    let credentials: Box<dyn CredentialStore> =
        Box::new(crate::adapters::jni::secure_store::AndroidCredentialStore);
    #[cfg(not(target_os = "android"))]
    let credentials: Box<dyn CredentialStore> =
        Box::new(crate::credentials::store::KeyringCredentialStore);
    Box::new(SplitSessionStorage::new(
        ConfigurationFile::new(configuration_path),
        credentials,
    ))
}

fn configuration_path() -> Result<PathBuf, String> {
    application_data_directory().map(|directory| directory.join("config.json"))
}

fn local_client_name() -> String {
    crate::domain::settings::format_client_name(device_name().as_deref())
}

#[cfg(target_os = "windows")]
fn device_name() -> Option<String> {
    #[link(name = "kernel32")]
    extern "system" {
        fn GetComputerNameW(buffer: *mut u16, size: *mut u32) -> i32;
    }

    let mut buffer = [0u16; 256];
    let mut size = buffer.len() as u32;
    if unsafe { GetComputerNameW(buffer.as_mut_ptr(), &mut size) } == 0 {
        return None;
    }
    String::from_utf16(&buffer[..size as usize]).ok()
}

#[cfg(target_os = "android")]
fn device_name() -> Option<String> {
    crate::adapters::jni::android_context::device_name().ok()
}

#[cfg(not(any(target_os = "windows", target_os = "android")))]
fn device_name() -> Option<String> {
    std::env::var("HOSTNAME").ok()
}

// App-owned data roots; executable installation and log rotation never own config.json.
#[cfg(test)]
pub(crate) fn application_data_directory() -> Result<PathBuf, String> {
    static ROOT: once_cell::sync::Lazy<PathBuf> = once_cell::sync::Lazy::new(|| {
        std::env::temp_dir().join(format!(
            "mashin-native-adapter-test-{}",
            uuid::Uuid::new_v4()
        ))
    });
    Ok(ROOT.clone())
}

#[cfg(all(not(test), target_os = "windows"))]
pub(crate) fn application_data_directory() -> Result<PathBuf, String> {
    std::env::var_os("LOCALAPPDATA")
        .map(|path| PathBuf::from(path).join("Mashin"))
        .ok_or_else(|| "LOCALAPPDATA is not set".to_string())
}

#[cfg(all(not(test), target_os = "android"))]
pub(crate) fn application_data_directory() -> Result<PathBuf, String> {
    crate::adapters::jni::android_context::external_application_files_directory()
}

#[cfg(all(not(test), any(target_os = "macos", target_os = "ios")))]
pub(crate) fn application_data_directory() -> Result<PathBuf, String> {
    std::env::var_os("HOME")
        .map(|path| PathBuf::from(path).join("Library/Application Support/Mashin"))
        .ok_or_else(|| "HOME is not set".to_string())
}

#[cfg(all(not(test), target_os = "linux"))]
pub(crate) fn application_data_directory() -> Result<PathBuf, String> {
    std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|path| PathBuf::from(path).join(".local/state")))
        .map(|path| path.join("mashin"))
        .ok_or_else(|| "neither XDG_STATE_HOME nor HOME is set".to_string())
}
