//! Blocking image normalization, bitmap validation and shared file publication.
//! Callers own generation checks and quiescence; no decoding or I/O belongs on an executor.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex};

use base64::Engine;
use cap_std::fs::{Dir, OpenOptions};
use image::ImageFormat;
use sha2::{Digest, Sha256};

use super::database::{validate_directory, validate_file, CacheError};
use super::storage::CacheStorage;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) struct ImageFileRecord {
    pub(super) content_hash: String,
    pub(super) mime_type: String,
    pub(super) byte_size: u64,
}

pub(super) struct ImageFiles {
    directory: Dir,
    path: PathBuf,
    leases: ImageFileLeases,
}

#[derive(Clone, Default)]
pub(super) struct ImageFileLeases {
    state: Arc<FileLeaseState>,
}

#[derive(Default)]
struct FileLeaseState {
    active: Mutex<usize>,
    released: Condvar,
}

#[derive(Clone)]
pub(super) struct ImageFileRead {
    pub(super) payload: ImagePayload,
    pub(super) lease: Arc<ImageFileLease>,
}

pub(super) struct PendingImageFileRead {
    record: ImageFileRecord,
    lease: ImageFileLease,
}

pub(super) struct ImageFileLease {
    file: Option<cap_std::fs::File>,
    leases: ImageFileLeases,
}

impl ImageFileLeases {
    /// Blocks only after admission has stopped and all workers have finished creating leases.
    pub(super) fn wait_until_released(&self) {
        let mut active = self
            .state
            .active
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        while *active != 0 {
            active = self
                .state
                .released
                .wait(active)
                .unwrap_or_else(|error| error.into_inner());
        }
    }
}

impl Drop for ImageFileLease {
    fn drop(&mut self) {
        drop(self.file.take());
        let mut active = self
            .leases
            .state
            .active
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        *active -= 1;
        self.leases.state.released.notify_all();
    }
}

impl PendingImageFileRead {
    pub(super) fn record(&self) -> ImageFileRecord {
        self.record.clone()
    }

    /// Validates off the index thread while retaining the admitted file handle.
    pub(super) fn validate(mut self) -> Result<Option<ImageFileRead>, CacheError> {
        let mut bytes = Vec::new();
        self.lease.file.as_mut().ok_or(CacheError::WorkerUnavailable)?.read_to_end(&mut bytes)?;
        if bytes.len() as u64 != self.record.byte_size
            || format!("{:x}", Sha256::digest(&bytes)) != self.record.content_hash
        {
            return Ok(None);
        }
        match ImagePayload::decode_bytes(bytes) {
            Ok(payload) if payload.mime_type == self.record.mime_type => {
                Ok(Some(ImageFileRead { payload, lease: Arc::new(self.lease) }))
            }
            _ => Ok(None),
        }
    }
}

impl ImageFiles {
    // Blocking capability initialization; the caller retains exclusive runtime ownership.
    pub(super) fn open(storage: &CacheStorage) -> Result<Self, CacheError> {
        Ok(Self {
            directory: storage.open_image_directory()?,
            path: storage.image_directory(),
            leases: ImageFileLeases::default(),
        })
    }

    /// Blocking crash recovery requires exclusive ownership with no active publications.
    pub(super) fn clear_incomplete_publications(&self) -> Result<(), CacheError> {
        for entry in self.directory.entries()? {
            let name = entry?.file_name();
            let Some(suffix) = name
                .to_str()
                .and_then(|name| name.strip_prefix(".tmp-"))
            else {
                continue;
            };
            if suffix.len() != 32 || !suffix.bytes().all(|value| value.is_ascii_hexdigit()) {
                continue;
            }
            validate_file(&self.path.join(&name))?;
            match self.directory.remove_file(&name) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        Ok(())
    }

    // Complete content is published before its caller may commit an index reference.
    pub(super) fn publish(&self, bytes: Vec<u8>) -> Result<ImageFileRecord, CacheError> {
        let payload = ImagePayload::decode_bytes(bytes)?;
        self.publish_payload(payload)
    }

    pub(super) fn publish_payload(&self, payload: ImagePayload) -> Result<ImageFileRecord, CacheError> {
        let record = ImageFileRecord {
            content_hash: format!("{:x}", Sha256::digest(&payload.bytes)),
            mime_type: payload.mime_type.to_owned(),
            byte_size: payload.bytes.len() as u64,
        };
        if self.read(&record)?.is_some() {
            return Ok(record);
        }
        let name = image_file_name(&record.content_hash, &record.mime_type)?;
        let shard = &record.content_hash[..2];
        match self.directory.create_dir(shard) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error.into()),
        }
        validate_directory(&self.path.join(shard))?;
        let shard_directory = self.directory.open_dir(shard)?;
        validate_file(&self.path.join(&name))?;
        let temporary = format!(".tmp-{}", uuid::Uuid::new_v4().simple());
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        let mut file = self.directory.open_with(&temporary, &options)?;
        let result = (|| {
            file.write_all(&payload.bytes)?;
            file.sync_all()?;
            drop(file);
            self.directory.rename(&temporary, &self.directory, &name)?;
            #[cfg(unix)]
            {
                shard_directory.into_std_file().sync_all()?;
                self.directory.try_clone()?.into_std_file().sync_all()?;
            }
            #[cfg(not(unix))]
            drop(shard_directory);
            Ok(record)
        })();
        match self.directory.remove_file(&temporary) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        result
    }

    // A missing or corrupt content file is a reparable miss, never successful image data.
    pub(super) fn read(
        &self,
        record: &ImageFileRecord,
    ) -> Result<Option<ImagePayload>, CacheError> {
        Ok(self.read_leased(record)?.map(|read| read.payload))
    }

    /// Keeps the verified file open until its consumer releases the returned lease.
    pub(super) fn read_leased(
        &self,
        record: &ImageFileRecord,
    ) -> Result<Option<ImageFileRead>, CacheError> {
        self.open_leased(record)?.map(PendingImageFileRead::validate).transpose().map(Option::flatten)
    }

    /// Admits a handle without reading or decoding its payload on the index owner.
    pub(super) fn open_leased(
        &self,
        record: &ImageFileRecord,
    ) -> Result<Option<PendingImageFileRead>, CacheError> {
        let name = image_file_name(&record.content_hash, &record.mime_type)?;
        match validate_directory(&self.path.join(&record.content_hash[..2])) {
            Ok(()) => {}
            Err(CacheError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(None);
            }
            Err(error) => return Err(error),
        }
        validate_file(&self.path.join(&name))?;
        let file = match self.directory.open(&name) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        if file.metadata()?.len() != record.byte_size {
            return Ok(None);
        }
        *self.leases.state.active.lock().unwrap_or_else(|error| error.into_inner()) += 1;
        Ok(Some(PendingImageFileRead {
            record: record.clone(),
            lease: ImageFileLease { file: Some(file), leases: self.leases.clone() },
        }))
    }

    pub(super) fn file_leases(&self) -> ImageFileLeases {
        self.leases.clone()
    }
}

#[derive(Clone)]
pub(super) struct ImagePayload {
    pub(super) bytes: Vec<u8>,
    pub(super) mime_type: &'static str,
}

impl ImagePayload {
    // Inline normalization validates actual content, not only the declared media type.
    pub(super) fn decode_inline(source: &str) -> Result<Self, CacheError> {
        if source.trim_start().starts_with('<') {
            return Self::decode_bytes(source.as_bytes().to_vec());
        }
        let (header, encoded) = source
            .split_once(',')
            .ok_or(CacheError::InvalidImageObservation("invalid data URI"))?;
        let declared = header
            .strip_prefix("data:")
            .and_then(|value| value.strip_suffix(";base64"))
            .ok_or(CacheError::InvalidImageObservation(
                "image requires a base64 data URI",
            ))?;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|_| CacheError::InvalidImageObservation("invalid image base64"))?;
        if declared == "image/svg+xml" {
            return Self::decode_bytes(super::image_svg::normalize(&bytes)?);
        }
        if image::guess_format(&bytes).is_err() {
            return Err(CacheError::InvalidImageObservation(
                "image MIME type contradicts content",
            ));
        }
        let payload = Self::decode_bytes(bytes)?;
        if declared != payload.mime_type {
            return Err(CacheError::InvalidImageObservation(
                "image MIME type contradicts content",
            ));
        }
        Ok(payload)
    }

    // Proxy and inline bytes share actual-format and safe-decode validation.
    pub(super) fn decode_bytes(bytes: Vec<u8>) -> Result<Self, CacheError> {
        let format = match image::guess_format(&bytes) {
            Ok(format) => format,
            Err(_) => return Self::decode_bytes(super::image_svg::normalize(&bytes)?),
        };
        let mime_type = match format {
            ImageFormat::Png => "image/png",
            ImageFormat::Jpeg => "image/jpeg",
            ImageFormat::WebP => "image/webp",
            ImageFormat::Gif => "image/gif",
            _ => {
                return Err(CacheError::InvalidImageObservation(
                    "unsupported bitmap format",
                ))
            }
        };
        let mut reader = image::ImageReader::with_format(std::io::Cursor::new(&bytes), format);
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(4096);
        limits.max_image_height = Some(4096);
        limits.max_alloc = Some(64 * 1024 * 1024);
        reader.limits(limits);
        reader
            .decode()
            .map_err(|_| CacheError::InvalidImageObservation("invalid or unsafe bitmap"))?;
        Ok(Self { bytes, mime_type })
    }
}

// Only content hashes enter file names; source references never become filesystem paths.
pub(super) fn image_file_name(content_hash: &str, mime_type: &str) -> Result<String, CacheError> {
    if content_hash.len() != 64
        || !content_hash
            .bytes()
            .all(|value| value.is_ascii_digit() || (b'a'..=b'f').contains(&value))
    {
        return Err(CacheError::InvalidImageObservation(
            "invalid image content hash",
        ));
    }
    let extension = match mime_type {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/webp" => "webp",
        "image/gif" => "gif",
        _ => return Err(CacheError::InvalidImageObservation("unsupported image MIME type")),
    };
    Ok(format!("{}/{content_hash}.{extension}", &content_hash[..2]))
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;

    pub(crate) fn bitmap(color: u8) -> Vec<u8> {
        let bitmap = image::RgbImage::from_pixel(2, 2, image::Rgb([color, 0, 0]));
        let mut encoded = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(bitmap)
            .write_to(&mut encoded, ImageFormat::Png)
            .unwrap();
        encoded.into_inner()
    }

    #[test]
    fn svg_and_inline_svg_share_normalized_png_bytes_without_mime_fallbacks() {
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="2" height="2"><path fill="red" d="M0 0H2V2H0Z"/></svg>"#;
        let encoded = base64::engine::general_purpose::STANDARD.encode(svg);
        let proxy = ImagePayload::decode_bytes(svg.to_vec()).unwrap();
        let inline =
            ImagePayload::decode_inline(&format!("data:image/svg+xml;base64,{encoded}")).unwrap();
        assert_eq!(proxy.mime_type, "image/png");
        assert_eq!(inline.bytes, proxy.bytes);
        assert_eq!(
            image::load_from_memory(&proxy.bytes)
                .unwrap()
                .to_rgba8()
                .get_pixel(0, 0)
                .0,
            [255, 0, 0, 255]
        );
        assert!(ImagePayload::decode_inline(&format!("data:image/png;base64,{encoded}")).is_err());
        let bitmap = base64::engine::general_purpose::STANDARD.encode(bitmap(1));
        assert!(
            ImagePayload::decode_inline(&format!("data:image/svg+xml;base64,{bitmap}")).is_err()
        );
        let application = tempfile::tempdir().unwrap();
        let storage = CacheStorage::open(application.path()).unwrap();
        let files = ImageFiles::open(&storage).unwrap();
        let record = files.publish(svg.to_vec()).unwrap();
        assert_eq!(record.mime_type, "image/png");
        assert_eq!(record, files.publish(inline.bytes).unwrap());
        assert_eq!(files.read(&record).unwrap().unwrap().bytes, proxy.bytes);
        assert_eq!(
            record.content_hash,
            format!("{:x}", Sha256::digest(&proxy.bytes))
        );
        drop(files);
        drop(storage);
        application.close().unwrap();
    }

    #[test]
    fn file_leases_hold_handles_until_release_without_retaining_byte_only_reads() {
        let application = tempfile::tempdir().unwrap();
        let storage = CacheStorage::open(application.path()).unwrap();
        let files = ImageFiles::open(&storage).unwrap();
        let record = files.publish(bitmap(7)).unwrap();
        let read = files.read_leased(&record).unwrap().unwrap();
        assert_eq!(read.payload.bytes, bitmap(7));
        let leases = files.file_leases();
        let (finished, completion) = std::sync::mpsc::channel();
        let waiting = std::thread::spawn(move || {
            leases.wait_until_released();
            finished.send(()).unwrap();
        });
        assert!(completion.try_recv().is_err());
        drop(read);
        completion
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap();
        waiting.join().unwrap();
        let payload = files.read(&record).unwrap().unwrap();
        files.file_leases().wait_until_released();
        assert_eq!(payload.bytes, bitmap(7));
        drop(files);
        drop(storage);
        application.close().unwrap();
    }

    #[test]
    fn published_bitmaps_survive_reopening_and_identical_bytes_share_one_file() {
        let application = tempfile::tempdir().unwrap();
        let storage = CacheStorage::open(application.path()).unwrap();
        let bytes = bitmap(7);
        let record = {
            let files = ImageFiles::open(&storage).unwrap();
            let record = files.publish(bytes.clone()).unwrap();
            assert_eq!(files.publish(bytes.clone()).unwrap(), record);
            assert_eq!(files.read(&record).unwrap().unwrap().bytes, bytes);
            assert_eq!(record.mime_type, "image/png");
            assert_eq!(record.byte_size, bytes.len() as u64);
            record
        };
        let files = ImageFiles::open(&storage).unwrap();
        assert_eq!(files.read(&record).unwrap().unwrap().bytes, bytes);
        let names = std::fs::read_dir(storage.image_directory())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert_eq!(names.len(), 1);
        assert!(names.contains(&record.content_hash[..2].to_owned()));
        let path = storage.image_directory().join(
            image_file_name(&record.content_hash, &record.mime_type).unwrap(),
        );
        assert_eq!(std::fs::read(path).unwrap(), bytes);
        drop(files);
        drop(storage);
        application.close().unwrap();
    }

    #[test]
    fn missing_corrupt_and_wrong_mime_content_is_a_repairable_miss() {
        let application = tempfile::tempdir().unwrap();
        let storage = CacheStorage::open(application.path()).unwrap();
        let files = ImageFiles::open(&storage).unwrap();
        let bytes = bitmap(1);
        let record = files.publish(bytes.clone()).unwrap();
        let path = storage
            .image_directory()
            .join(image_file_name(&record.content_hash, &record.mime_type).unwrap());
        let mut wrong_mime = record.clone();
        wrong_mime.mime_type = "image/jpeg".to_owned();
        assert!(files.read(&wrong_mime).unwrap().is_none());
        std::fs::write(&path, vec![0; bytes.len()]).unwrap();
        assert!(files.read(&record).unwrap().is_none());
        assert_eq!(files.publish(bytes.clone()).unwrap(), record);
        assert_eq!(files.read(&record).unwrap().unwrap().bytes, bytes);
        std::fs::remove_file(&path).unwrap();
        assert!(files.read(&record).unwrap().is_none());
        files.publish(bytes.clone()).unwrap();
        assert_eq!(files.read(&record).unwrap().unwrap().bytes, bytes);
        drop(files);
        drop(storage);
        application.close().unwrap();
    }

    #[test]
    fn failed_publication_and_invalid_bytes_leave_no_temporary_or_partial_files() {
        let application = tempfile::tempdir().unwrap();
        let storage = CacheStorage::open(application.path()).unwrap();
        let files = ImageFiles::open(&storage).unwrap();
        assert!(files.publish(b"not an image".to_vec()).is_err());
        let bytes = bitmap(2);
        let hash = format!("{:x}", Sha256::digest(&bytes));
        let destination = storage
            .image_directory()
            .join(image_file_name(&hash, "image/png").unwrap());
        std::fs::create_dir_all(&destination).unwrap();
        assert!(files.publish(bytes).is_err());
        assert!(!std::fs::read_dir(storage.image_directory())
            .unwrap()
            .any(|entry| entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".tmp-")));
        assert_eq!(std::fs::read_dir(&destination).unwrap().count(), 0);
        assert!(image_file_name("../../credentials", "image/png").is_err());
        drop(files);
        drop(storage);
        application.close().unwrap();
    }

    #[test]
    fn inline_images_validate_bitmap_content_and_reject_active_or_mislabeled_data() {
        let application = tempfile::tempdir().unwrap();
        let storage = CacheStorage::open(application.path()).unwrap();
        let files = ImageFiles::open(&storage).unwrap();
        for (format, mime_type, extension) in [
            (ImageFormat::Png, "image/png", "png"),
            (ImageFormat::Jpeg, "image/jpeg", "jpg"),
            (ImageFormat::WebP, "image/webp", "webp"),
            (ImageFormat::Gif, "image/gif", "gif"),
        ] {
            let mut bytes = std::io::Cursor::new(Vec::new());
            image::DynamicImage::ImageRgb8(image::RgbImage::new(2, 2))
                .write_to(&mut bytes, format)
                .unwrap();
            let bytes = bytes.into_inner();
            let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);
            let payload =
                ImagePayload::decode_inline(&format!("data:{mime_type};base64,{encoded}")).unwrap();
            assert_eq!(payload.bytes, bytes);
            assert_eq!(payload.mime_type, mime_type);
            let proxy_payload = ImagePayload::decode_bytes(bytes.clone()).unwrap();
            assert_eq!(proxy_payload.bytes, payload.bytes);
            assert_eq!(proxy_payload.mime_type, payload.mime_type);
            let record = files.publish(bytes.clone()).unwrap();
            let relative = format!(
                "{}/{}.{extension}", &record.content_hash[..2], record.content_hash,
            );
            assert_eq!(
                std::fs::read(storage.image_directory().join(relative)).unwrap(),
                bytes,
            );
            assert_eq!(files.read(&record).unwrap().unwrap().bytes, bytes);
            assert!(
                ImagePayload::decode_inline(&format!("data:image/bmp;base64,{encoded}")).is_err()
            );
            assert!(ImagePayload::decode_inline(&format!(
                "data:{mime_type};base64,{}",
                base64::engine::general_purpose::STANDARD.encode(&bytes[..bytes.len() / 2])
            ))
            .is_err());
        }
        for source in [
            "not-a-data-uri",
            "data:image/png;base64,",
            "data:image/png;base64,%%%",
            "data:image/png,raw",
            "data:text/html;base64,PHNjcmlwdD4=",
            "data:image/svg+xml;base64,PHN2Zz48c2NyaXB0Lz48L3N2Zz4=",
        ] {
            assert!(ImagePayload::decode_inline(source).is_err());
        }
        drop(files);
        drop(storage);
        application.close().unwrap();
    }

    #[cfg(any(windows, unix))]
    #[test]
    fn redirected_hash_shards_cannot_read_publish_or_delete_external_images() {
        let application = tempfile::tempdir().unwrap();
        let storage = CacheStorage::open(application.path()).unwrap();
        let files = ImageFiles::open(&storage).unwrap();
        let retained = files.publish(bitmap(7)).unwrap();
        let protected = tempfile::tempdir().unwrap();
        let bytes = bitmap(6);
        let record = ImageFileRecord {
            content_hash: format!("{:x}", Sha256::digest(&bytes)),
            mime_type: "image/png".to_owned(),
            byte_size: bytes.len() as u64,
        };
        assert_ne!(&retained.content_hash[..2], &record.content_hash[..2]);
        let relative = PathBuf::from(image_file_name(&record.content_hash, &record.mime_type).unwrap());
        let external = protected.path().join(relative.file_name().unwrap());
        std::fs::write(&external, &bytes).unwrap();
        let redirected = storage.image_directory().join(&record.content_hash[..2]);
        #[cfg(windows)]
        {
            let output = std::process::Command::new("cmd")
                .args(["/C", "mklink", "/J"])
                .arg(&redirected)
                .arg(protected.path())
                .output()
                .unwrap();
            assert!(output.status.success(), "{output:?}");
        }
        #[cfg(unix)]
        std::os::unix::fs::symlink(protected.path(), &redirected).unwrap();
        assert!(matches!(files.read(&record), Err(CacheError::UnsafePath(_))));
        assert!(matches!(files.publish(bytes.clone()), Err(CacheError::UnsafePath(_))));
        assert!(matches!(storage.clear_image_files(), Err(CacheError::UnsafePath(_))));
        assert_eq!(std::fs::read(external).unwrap(), bytes);
        assert_eq!(files.read(&retained).unwrap().unwrap().bytes, bitmap(7));
        #[cfg(windows)]
        std::fs::remove_dir(redirected).unwrap();
        #[cfg(unix)]
        std::fs::remove_file(redirected).unwrap();
        drop(files);
        drop(storage);
        application.close().unwrap();
        protected.close().unwrap();
    }
}
