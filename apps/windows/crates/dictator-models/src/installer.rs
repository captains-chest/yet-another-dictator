//! Downloads a model listed in the bundled manifest into `<root>/<model-id>/`, verifying every
//! file's size and SHA-256 before it is moved into place. Resumes from `.part` files and retries with
//! backoff. A `.complete` marker, written last, tells the loader the folder is whole.

use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use dictator_core::StopSignal;
use sha2::{Digest, Sha256};

use crate::manifest::{Entry, ModelManifest};

pub const DEFAULT_MAX_ATTEMPTS: u32 = 5;
const MARKER: &str = ".complete";
const HASH_CHUNK_BYTES: usize = 4 << 20;
const CANCEL_POLL: Duration = Duration::from_millis(100);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallError {
    ChecksumMismatch(String),
    UnknownModel(String),
    Cancelled,
    /// The download failed (network, server); worth retrying.
    Transport(String),
    Io(String),
}

impl fmt::Display for InstallError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ChecksumMismatch(file) => {
                write!(f, "Downloaded file failed verification: {file}")
            }
            Self::UnknownModel(id) => write!(f, "Model {id} is not in the manifest"),
            Self::Cancelled => f.write_str("Cancelled"),
            Self::Transport(message) | Self::Io(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for InstallError {}

impl From<io::Error> for InstallError {
    fn from(error: io::Error) -> Self {
        Self::Io(error.to_string())
    }
}

/// Fetches one manifest file's bytes into a `.part` file, resuming from what is already there.
/// The installer verifies the result; a transport only moves bytes.
pub trait ModelTransport: Send + Sync {
    /// Appends the missing bytes of `entry` to `part`, which may already hold a prefix. Calls
    /// `progress` with the number of bytes in `part` so far. Returns `Cancelled` once `stop` is set.
    fn fetch(
        &self,
        entry: &Entry,
        part: &Path,
        progress: &mut dyn FnMut(u64),
        stop: &StopSignal,
    ) -> Result<(), InstallError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    pub completed_bytes: u64,
    pub total_bytes: u64,
}

impl Progress {
    pub fn fraction(&self) -> f64 {
        if self.total_bytes > 0 {
            self.completed_bytes as f64 / self.total_bytes as f64
        } else {
            0.0
        }
    }
}

/// `%LOCALAPPDATA%\Dictator\Models`.
pub fn default_root() -> Option<PathBuf> {
    default_root_from(|key| std::env::var_os(key))
}

/// [`default_root`] with an injectable environment. Falls back to `%USERPROFILE%\AppData\Local`,
/// then `$HOME/.local/share` (development on other systems).
pub fn default_root_from(env: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    let present = |key: &str| env(key).filter(|v| !v.is_empty()).map(PathBuf::from);
    let base = present("LOCALAPPDATA")
        .or_else(|| present("USERPROFILE").map(|p| p.join("AppData").join("Local")))
        .or_else(|| present("HOME").map(|p| p.join(".local").join("share")))?;
    Some(base.join("Dictator").join("Models"))
}

#[derive(Debug, Clone)]
pub struct ModelInstaller {
    pub manifest: ModelManifest,
    pub model_id: String,
    pub root: PathBuf,
    pub max_attempts: u32,
}

impl ModelInstaller {
    pub fn new(
        manifest: ModelManifest,
        model_id: impl Into<String>,
        root: impl Into<PathBuf>,
    ) -> Self {
        Self {
            manifest,
            model_id: model_id.into(),
            root: root.into(),
            max_attempts: DEFAULT_MAX_ATTEMPTS,
        }
    }

    pub fn folder(&self) -> PathBuf {
        self.root.join(&self.model_id)
    }

    fn marker(&self) -> PathBuf {
        self.folder().join(MARKER)
    }

    pub fn total_bytes(&self) -> u64 {
        self.manifest.total_bytes(&self.model_id)
    }

    /// The marker holds a digest of this model's manifest lines, so a model bump means re-download.
    fn manifest_digest(&self) -> String {
        let lines: Vec<String> = self
            .manifest
            .files(&self.model_id)
            .iter()
            .map(|e| format!("{} {}", e.sha256, e.local_path))
            .collect();
        hex(&Sha256::digest(lines.join("\n").as_bytes()))
    }

    pub fn is_installed(&self) -> bool {
        fs::read_to_string(self.marker()).is_ok_and(|text| text == self.manifest_digest())
    }

    /// Downloads whatever is missing. Safe to call again after a failure or a quit: it resumes.
    pub fn install(
        &self,
        transport: &dyn ModelTransport,
        progress: &mut dyn FnMut(Progress),
        stop: &StopSignal,
    ) -> Result<(), InstallError> {
        let files = self.manifest.files(&self.model_id);
        if files.is_empty() {
            return Err(InstallError::UnknownModel(self.model_id.clone()));
        }
        if self.is_installed() {
            return Ok(());
        }
        self.remove_stale_models()?;
        let folder = self.folder();
        fs::create_dir_all(&folder)?;
        let _ = fs::remove_file(self.marker());

        let total = self.total_bytes();
        let mut done = 0;
        for entry in files {
            let target = folder.join(&entry.local_path);
            if !is_verified(&target, entry)? {
                let base = done;
                self.fetch_with_retry(
                    entry,
                    &target,
                    transport,
                    &mut |written| {
                        progress(Progress {
                            completed_bytes: base + written,
                            total_bytes: total,
                        })
                    },
                    stop,
                )?;
            }
            done += entry.bytes;
            progress(Progress {
                completed_bytes: done,
                total_bytes: total,
            });
        }
        let temporary = folder.join(format!("{MARKER}.tmp"));
        fs::write(&temporary, self.manifest_digest())?;
        fs::rename(temporary, self.marker())?;
        Ok(())
    }

    /// Deletes this model's folder (the user switched models and chose to free the space).
    pub fn uninstall(&self) -> io::Result<()> {
        match fs::remove_dir_all(self.folder()) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            result => result,
        }
    }

    /// Deletes model folders that are no longer in the manifest.
    pub fn remove_stale_models(&self) -> io::Result<()> {
        let keep = self.manifest.model_ids();
        let Ok(children) = fs::read_dir(&self.root) else {
            return Ok(());
        };
        for child in children.flatten() {
            if keep.iter().any(|id| child.file_name() == **id) {
                continue;
            }
            let path = child.path();
            let _ = if path.is_dir() {
                fs::remove_dir_all(path)
            } else {
                fs::remove_file(path)
            };
        }
        Ok(())
    }

    fn fetch_with_retry(
        &self,
        entry: &Entry,
        target: &Path,
        transport: &dyn ModelTransport,
        progress: &mut dyn FnMut(u64),
        stop: &StopSignal,
    ) -> Result<(), InstallError> {
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut part = target.as_os_str().to_owned();
        part.push(".part");
        let part = PathBuf::from(part);
        let mut attempt = 0;
        loop {
            attempt += 1;
            match self.fetch_once(entry, target, &part, transport, progress, stop) {
                Ok(()) => return Ok(()),
                Err(InstallError::Cancelled) => return Err(InstallError::Cancelled),
                Err(error) if attempt >= self.max_attempts => return Err(error),
                // 2, 4, 8, 16 s
                Err(_) => sleep(Duration::from_secs(1 << attempt), stop)?,
            }
        }
    }

    fn fetch_once(
        &self,
        entry: &Entry,
        target: &Path,
        part: &Path,
        transport: &dyn ModelTransport,
        progress: &mut dyn FnMut(u64),
        stop: &StopSignal,
    ) -> Result<(), InstallError> {
        if stop.is_set() {
            return Err(InstallError::Cancelled);
        }
        transport.fetch(entry, part, progress, stop)?;
        if fs::metadata(part)?.len() != entry.bytes || sha256(part)? != entry.sha256 {
            let _ = fs::remove_file(part);
            return Err(InstallError::ChecksumMismatch(entry.local_path.clone()));
        }
        let _ = fs::remove_file(target);
        fs::rename(part, target)?;
        Ok(())
    }
}

/// A file already in place counts only if size and hash match the manifest.
fn is_verified(path: &Path, entry: &Entry) -> io::Result<bool> {
    match fs::metadata(path) {
        Ok(metadata) if metadata.is_file() && metadata.len() == entry.bytes => {
            Ok(sha256(path)? == entry.sha256)
        }
        _ => Ok(false),
    }
}

fn sha256(path: &Path) -> io::Result<String> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0; HASH_CHUNK_BYTES];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            return Ok(hex(&hasher.finalize()));
        }
        hasher.update(&buffer[..read]);
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Sleeps for `duration` unless `stop` is set first.
fn sleep(duration: Duration, stop: &StopSignal) -> Result<(), InstallError> {
    let end = Instant::now() + duration;
    while Instant::now() < end {
        if stop.is_set() {
            return Err(InstallError::Cancelled);
        }
        std::thread::sleep(CANCEL_POLL.min(end.saturating_duration_since(Instant::now())));
    }
    Ok(())
}
