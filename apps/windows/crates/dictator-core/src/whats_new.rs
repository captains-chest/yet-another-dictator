//! Port of `WhatsNew.swift`: parses CHANGELOG.md and decides whether to show What's New.

use crate::app_version::AppVersion;
use crate::text::trim_horizontal;

/// One version's section of CHANGELOG.md.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangelogEntry {
    pub version: AppVersion,
    /// The heading's remainder, e.g. the date ("2026-10-09"); may be empty.
    pub note: String,
    /// The section's Markdown, without its heading.
    pub body: String,
}

/// Parses `## [0.1.0] - 2026-10-09` / `## 0.1.0` sections, newest first as written.
/// Sections whose heading isn't a version (e.g. "Unreleased") are skipped.
pub fn parse_changelog(markdown: &str) -> Vec<ChangelogEntry> {
    let mut entries = Vec::new();
    let mut current: Option<(AppVersion, String)> = None;
    let mut lines: Vec<&str> = Vec::new();

    let mut flush = |current: &Option<(AppVersion, String)>, lines: &mut Vec<&str>| {
        if let Some((version, note)) = current {
            let body = lines.join("\n").trim().to_string();
            entries.push(ChangelogEntry {
                version: version.clone(),
                note: note.clone(),
                body,
            });
        }
        lines.clear();
    };

    for line in markdown.split('\n') {
        if let Some(heading_text) = line.strip_prefix("## ") {
            flush(&current, &mut lines);
            current = heading(heading_text);
        } else if current.is_some() {
            lines.push(line);
        }
    }
    flush(&current, &mut lines);
    entries
}

fn heading(text: &str) -> Option<(AppVersion, String)> {
    let trimmed = trim_horizontal(text);
    let token_end = trimmed.find(char::is_whitespace).unwrap_or(trimmed.len());
    let token = &trimmed[..token_end];
    let version = AppVersion::parse(token.trim_matches(['[', ']']))?;
    let note = trimmed[token_end..].trim_matches([' ', '-', '–', '—']);
    Some((version, note.to_string()))
}

/// Whether to show the What's New window at launch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// Nothing to show (same version as last time, or a downgrade).
    None,
    /// First launch ever (nothing seen before): a welcome with the current version's notes.
    Welcome(Vec<ChangelogEntry>),
    /// Updated: the notes of every version after the last one seen, up to this one.
    Updated(Vec<ChangelogEntry>),
}

pub fn decide(
    current: &AppVersion,
    last_seen: Option<&str>,
    changelog: &[ChangelogEntry],
) -> Decision {
    let this_version = || {
        changelog
            .iter()
            .filter(|e| e.version == *current)
            .cloned()
            .collect()
    };
    let Some(last_seen) = last_seen else {
        return Decision::Welcome(this_version());
    };
    let Some(seen) = AppVersion::parse(last_seen) else {
        // Unreadable stored value: treat as an update, showing this version's notes.
        return Decision::Updated(this_version());
    };
    if seen >= *current {
        return Decision::None;
    }
    let mut entries: Vec<ChangelogEntry> = changelog
        .iter()
        .filter(|e| seen < e.version && e.version <= *current)
        .cloned()
        .collect();
    entries.sort_by(|a, b| b.version.cmp(&a.version));
    Decision::Updated(entries)
}
