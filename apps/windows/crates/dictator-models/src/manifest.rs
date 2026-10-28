//! The pinned list of model files the app may download. It ships next to `Dictator.exe` inside the
//! signed install and is the only trust anchor: it is never fetched from the network.
//!
//! Line format: `<model-id>  <sha256>  <bytes>  <huggingface-path>  <local-path>`; `#` starts a comment.

use std::collections::HashSet;
use std::fmt;
use std::io;
use std::path::Path;
use std::str::FromStr;

use dictator_core::text::{is_newline, trim_horizontal};

pub const FILE_NAME: &str = "model-manifest.txt";
const SHA256_HEX_LENGTH: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub model_id: String,
    /// Lowercase hex.
    pub sha256: String,
    pub bytes: u64,
    pub remote_path: String,
    pub local_path: String,
}

impl Entry {
    pub fn url(&self) -> String {
        format!("https://huggingface.co/{}", self.remote_path)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    Malformed { line: String },
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Malformed { line } => write!(f, "Malformed model manifest line: {line}"),
        }
    }
}

impl std::error::Error for ParseError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelManifest {
    entries: Vec<Entry>,
}

impl ModelManifest {
    pub fn parse(text: &str) -> Result<Self, ParseError> {
        let entries = text
            .split(is_newline)
            .map(trim_horizontal)
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .map(|line| {
                parse_line(line).ok_or_else(|| ParseError::Malformed {
                    line: line.to_string(),
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut seen = HashSet::new();
        if !entries
            .iter()
            .all(|e| seen.insert((e.model_id.as_str(), e.local_path.as_str())))
        {
            return Err(ParseError::Malformed {
                line: "duplicate entry".into(),
            });
        }
        Ok(Self { entries })
    }

    /// The manifest shipped next to the running executable.
    pub fn bundled() -> io::Result<Self> {
        let exe = std::env::current_exe()?;
        let folder = exe.parent().unwrap_or(Path::new("."));
        Self::read(&folder.join(FILE_NAME))
    }

    pub fn read(path: &Path) -> io::Result<Self> {
        let text = std::fs::read_to_string(path)?;
        Self::parse(&text).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// Every model id, in manifest order.
    pub fn model_ids(&self) -> Vec<&str> {
        let mut seen = HashSet::new();
        self.entries
            .iter()
            .map(|e| e.model_id.as_str())
            .filter(|id| seen.insert(*id))
            .collect()
    }

    pub fn files(&self, model_id: &str) -> Vec<&Entry> {
        self.entries
            .iter()
            .filter(|e| e.model_id == model_id)
            .collect()
    }

    pub fn total_bytes(&self, model_id: &str) -> u64 {
        self.files(model_id).iter().map(|e| e.bytes).sum()
    }
}

impl FromStr for ModelManifest {
    type Err = ParseError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        Self::parse(text)
    }
}

fn parse_line(line: &str) -> Option<Entry> {
    let [model_id, sha256, bytes, remote_path, local_path] = <[&str; 5]>::try_from(
        line.split(char::is_whitespace)
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>(),
    )
    .ok()?;
    let is_sha = sha256.len() == SHA256_HEX_LENGTH
        && sha256
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
    let bytes = bytes.parse::<i64>().ok().filter(|&b| b > 0)?;
    (is_sha && is_safe(remote_path) && is_safe(local_path)).then(|| Entry {
        model_id: model_id.into(),
        sha256: sha256.into(),
        bytes: bytes as u64,
        remote_path: remote_path.into(),
        local_path: local_path.into(),
    })
}

/// Relative, no `..` or empty segments, and only ASCII letters, digits and `._-/`.
fn is_safe(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && path
            .split('/')
            .all(|segment| !segment.is_empty() && segment != "..")
        && path
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "._-/".contains(c))
}
