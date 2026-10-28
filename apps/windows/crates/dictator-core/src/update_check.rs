//! Port of `UpdateCheck.swift`. JSON decoding is left to the shell (this crate has no
//! dependencies): it fills [`ReleaseInfo`] from GitHub's `tag_name`, `html_url`, `draft` and
//! `prerelease` fields.

use std::time::{Duration, SystemTime};

use crate::app_version::AppVersion;

/// One entry of GitHub's `GET /repos/{owner}/{repo}/releases` response (only the fields used).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseInfo {
    pub tag_name: String,
    pub html_url: String,
    pub draft: bool,
    pub prerelease: bool,
}

impl ReleaseInfo {
    pub fn new(tag_name: &str, html_url: &str) -> Self {
        Self {
            tag_name: tag_name.into(),
            html_url: html_url.into(),
            draft: false,
            prerelease: false,
        }
    }
}

/// A newer release the user can download from its release page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AvailableUpdate {
    pub version: AppVersion,
    pub page_url: String,
}

pub const INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

/// Foundation's `URL(string:)` rejects whitespace and control characters; the scheme must be https.
fn is_https_url(url: &str) -> bool {
    url.split_once(':')
        .is_some_and(|(scheme, rest)| scheme == "https" && !rest.is_empty())
        && !url.chars().any(|c| c.is_whitespace() || c.is_control())
}

/// The newest release above `current`, or `None` if there is none.
///
/// GitHub's `releases/latest` skips pre-releases, so the full list is used instead. Drafts and
/// tags that aren't versions are ignored. Pre-releases (marked on GitHub or by a `-beta`-style
/// tag) count while Dictator is below 1.0, where every release is a pre-release, or when the
/// running version is itself a pre-release; from 1.0 on, stable builds only see stable releases.
pub fn newest(releases: &[ReleaseInfo], current: &AppVersion) -> Option<AvailableUpdate> {
    let accepts_prereleases = current.major < 1 || current.is_prerelease();
    releases
        .iter()
        .filter(|release| !release.draft && is_https_url(&release.html_url))
        .filter_map(|release| {
            let version = AppVersion::parse(&release.tag_name)?;
            if !accepts_prereleases && (release.prerelease || version.is_prerelease()) {
                return None;
            }
            Some(AvailableUpdate {
                version,
                page_url: release.html_url.clone(),
            })
        })
        .filter(|update| *current < update.version)
        .max_by(|a, b| a.version.cmp(&b.version))
}

/// Whether the daily automatic check should run now.
pub fn is_due(last_check: Option<SystemTime>, now: SystemTime) -> bool {
    let Some(last_check) = last_check else {
        return true;
    };
    // A clock set backwards also triggers a check rather than waiting indefinitely.
    match now.duration_since(last_check) {
        Ok(elapsed) => elapsed >= INTERVAL,
        Err(_) => true,
    }
}
