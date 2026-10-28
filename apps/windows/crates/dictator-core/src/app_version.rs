//! Port of `AppVersion.swift`.

use std::cmp::Ordering;
use std::fmt;

use crate::text::trim_horizontal;

/// A semantic version like `0.1.0` or `v0.2.0-beta.1`, compared by semver rules (a pre-release
/// sorts before its release; build metadata is ignored).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AppVersion {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    /// Dot-separated pre-release identifiers, e.g. `["beta", "1"]`; empty for a release.
    pub prerelease: Vec<String>,
}

/// Swift's `Int(_:)`: an optional sign, then ASCII digits, within `i64`.
fn swift_int(text: &str) -> Option<i64> {
    text.parse().ok()
}

impl AppVersion {
    pub fn parse(string: &str) -> Option<Self> {
        let mut text = trim_horizontal(string);
        if let Some(rest) = text.strip_prefix(['v', 'V']) {
            text = rest;
        }
        if let Some(plus) = text.find('+') {
            text = &text[..plus];
        }
        let mut prerelease = Vec::new();
        if let Some(dash) = text.find('-') {
            prerelease = text[dash + 1..].split('.').map(String::from).collect();
            if prerelease.iter().any(String::is_empty) {
                return None;
            }
            text = &text[..dash];
        }
        let numbers: Vec<&str> = text.split('.').collect();
        if !(1..=3).contains(&numbers.len()) {
            return None;
        }
        let mut parts = Vec::with_capacity(3);
        for number in numbers {
            if number.is_empty() || !number.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            parts.push(u64::try_from(swift_int(number)?).ok()?);
        }
        parts.resize(3, 0);
        Some(Self {
            major: parts[0],
            minor: parts[1],
            patch: parts[2],
            prerelease,
        })
    }

    pub fn is_prerelease(&self) -> bool {
        !self.prerelease.is_empty()
    }

    fn precedes(&self, other: &Self) -> bool {
        let left = [self.major, self.minor, self.patch];
        let right = [other.major, other.minor, other.patch];
        if left != right {
            return left < right;
        }
        match (self.prerelease.is_empty(), other.prerelease.is_empty()) {
            (true, _) => return false,
            (false, true) => return true,
            (false, false) => {}
        }
        for (a, b) in self.prerelease.iter().zip(&other.prerelease) {
            if a == b {
                continue;
            }
            return match (swift_int(a), swift_int(b)) {
                (Some(x), Some(y)) => x < y,
                // Numeric identifiers sort first.
                (Some(_), None) => true,
                (None, Some(_)) => false,
                (None, None) => a < b,
            };
        }
        self.prerelease.len() < other.prerelease.len()
    }
}

impl std::str::FromStr for AppVersion {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, ()> {
        Self::parse(s).ok_or(())
    }
}

impl Ord for AppVersion {
    fn cmp(&self, other: &Self) -> Ordering {
        if self.precedes(other) {
            Ordering::Less
        } else if other.precedes(self) {
            Ordering::Greater
        } else {
            Ordering::Equal
        }
    }
}

impl PartialOrd for AppVersion {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for AppVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)?;
        if !self.prerelease.is_empty() {
            write!(f, "-{}", self.prerelease.join("."))?;
        }
        Ok(())
    }
}
