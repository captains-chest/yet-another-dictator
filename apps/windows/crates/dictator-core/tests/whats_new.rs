//! Port of WhatsNewTests.swift (AppVersion and Changelog tests).

use dictator_core::app_version::AppVersion;
use dictator_core::whats_new::{self, ChangelogEntry, Decision, parse_changelog};

fn v(text: &str) -> AppVersion {
    AppVersion::parse(text).expect(text)
}

mod app_version_tests {
    use super::*;

    #[test]
    fn parses_common_forms() {
        let version = v("v1.2.3");
        assert_eq!((version.major, version.minor, version.patch), (1, 2, 3));
        assert_eq!(v("0.1").to_string(), "0.1.0");
        assert_eq!(v("0.2.0-beta.1+abc").to_string(), "0.2.0-beta.1");
    }

    #[test]
    fn rejects_garbage() {
        for text in [
            "",
            "v",
            "1.2.3.4",
            "1..2",
            "1.x",
            "1.2.3-",
            "1.2.3-a..b",
            "latest",
        ] {
            assert_eq!(AppVersion::parse(text), None, "{text:?}");
        }
    }

    #[test]
    fn compares_by_semver() {
        let ordered = [
            "0.0.1",
            "0.1.0-alpha",
            "0.1.0-alpha.1",
            "0.1.0-alpha.beta",
            "0.1.0-beta.2",
            "0.1.0-beta.11",
            "0.1.0",
            "0.1.1",
            "0.10.0",
            "1.0.0",
        ];
        let versions: Vec<AppVersion> = ordered.iter().map(|t| v(t)).collect();
        for pair in versions.windows(2) {
            assert!(pair[0] < pair[1], "{} < {}", pair[0], pair[1]);
            assert!(pair[1] >= pair[0]);
        }
        assert_eq!(v("v0.1.0"), v("0.1.0+build.7"));
    }
}

const MARKDOWN: &str = "# Changelog

## [Unreleased]
- Not out yet.

## [0.2.0] - 2026-11-01
### Added
- Thing two.

## [0.1.0] - 2026-10-09
### Added
- Thing one.

## 0.0.1
First build.";

fn entries() -> Vec<ChangelogEntry> {
    parse_changelog(MARKDOWN)
}

fn versions(entries: &[ChangelogEntry]) -> Vec<String> {
    entries.iter().map(|e| e.version.to_string()).collect()
}

#[test]
fn parses_version_sections() {
    let entries = entries();
    assert_eq!(versions(&entries), ["0.2.0", "0.1.0", "0.0.1"]);
    assert_eq!(entries[0].note, "2026-11-01");
    assert_eq!(entries[0].body, "### Added\n- Thing two.");
    assert_eq!(entries[2].note, "");
    assert_eq!(entries[2].body, "First build.");
}

#[test]
fn same_version_shows_nothing() {
    assert_eq!(
        whats_new::decide(&v("0.2.0"), Some("0.2.0"), &entries()),
        Decision::None
    );
}

#[test]
fn downgrade_shows_nothing() {
    assert_eq!(
        whats_new::decide(&v("0.1.0"), Some("0.2.0"), &entries()),
        Decision::None
    );
}

#[test]
fn update_shows_every_version_since_last_seen() {
    let Decision::Updated(shown) = whats_new::decide(&v("0.2.0"), Some("0.0.1"), &entries()) else {
        panic!("expected an update");
    };
    assert_eq!(versions(&shown), ["0.2.0", "0.1.0"]);
}

#[test]
fn fresh_install_gets_a_welcome_with_this_version() {
    let Decision::Welcome(shown) = whats_new::decide(&v("0.1.0"), None, &entries()) else {
        panic!("expected a welcome");
    };
    assert_eq!(versions(&shown), ["0.1.0"]);
}

#[test]
fn unreadable_last_seen_shows_this_version() {
    let entries = entries();
    assert_eq!(
        whats_new::decide(&v("0.2.0"), Some("garbage"), &entries),
        Decision::Updated(vec![entries[0].clone()])
    );
}

/// The Windows app ships the repo's CHANGELOG.md; it must parse and hold the workspace version.
#[test]
fn the_repo_changelog_parses_and_has_this_version() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../../../CHANGELOG.md");
    let markdown = std::fs::read_to_string(path).expect("CHANGELOG.md");
    let entries = parse_changelog(&markdown);
    assert!(!entries.is_empty());
    assert!(
        entries
            .iter()
            .any(|e| e.version == v(env!("CARGO_PKG_VERSION")))
    );
}
