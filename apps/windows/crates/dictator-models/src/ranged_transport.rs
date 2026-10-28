//! The real transport: HTTPS Range requests in fixed-size chunks, appended to the `.part` file.
//! Resumes after a failure or a relaunch from the bytes already on disk. No cookies, no cache.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;
use std::time::Duration;

use dictator_core::StopSignal;

use crate::installer::{InstallError, ModelTransport};
use crate::manifest::Entry;

pub const DEFAULT_CHUNK_BYTES: u64 = 8 << 20;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
const HTTP_OK: u16 = 200;
const HTTP_PARTIAL_CONTENT: u16 = 206;

pub struct RangedTransport {
    agent: ureq::Agent,
    chunk_bytes: u64,
}

impl Default for RangedTransport {
    fn default() -> Self {
        Self::new(DEFAULT_CHUNK_BYTES)
    }
}

impl RangedTransport {
    pub fn new(chunk_bytes: u64) -> Self {
        let agent = ureq::Agent::config_builder()
            .https_only(true)
            .timeout_global(Some(REQUEST_TIMEOUT))
            .user_agent("Dictator")
            .build()
            .into();
        Self {
            agent,
            chunk_bytes: chunk_bytes.max(1),
        }
    }

    fn get(
        &self,
        url: &str,
        offset: u64,
        end: u64,
        entry_bytes: u64,
    ) -> Result<Vec<u8>, InstallError> {
        let failed = |e: ureq::Error| InstallError::Transport(e.to_string());
        let response = self
            .agent
            .get(url)
            .header("Range", &format!("bytes={offset}-{end}"))
            .call()
            .map_err(failed)?;
        let requested = end - offset + 1;
        let expected = match response.status().as_u16() {
            HTTP_PARTIAL_CONTENT => requested,
            HTTP_OK if offset == 0 => entry_bytes, // Small file served whole.
            status => {
                return Err(InstallError::Transport(format!(
                    "Unexpected server response ({status})"
                )));
            }
        };
        let data = response
            .into_body()
            .into_with_config()
            .limit(expected)
            .read_to_vec()
            .map_err(failed)?;
        if data.len() as u64 != expected {
            return Err(InstallError::Transport("Incomplete server response".into()));
        }
        Ok(data)
    }
}

impl ModelTransport for RangedTransport {
    fn fetch(
        &self,
        entry: &Entry,
        part: &Path,
        progress: &mut dyn FnMut(u64),
        stop: &StopSignal,
    ) -> Result<(), InstallError> {
        let mut file = OpenOptions::new().create(true).append(true).open(part)?;
        let mut offset = file.metadata()?.len();
        if offset > entry.bytes {
            file.set_len(0)?;
            offset = 0;
        }
        let url = entry.url();
        while offset < entry.bytes {
            if stop.is_set() {
                return Err(InstallError::Cancelled);
            }
            let end = (offset + self.chunk_bytes).min(entry.bytes) - 1;
            let data = self.get(&url, offset, end, entry.bytes)?;
            file.write_all(&data)?;
            offset += data.len() as u64;
            progress(offset);
        }
        Ok(())
    }
}
