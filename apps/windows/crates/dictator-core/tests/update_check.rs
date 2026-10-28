//! Port of UpdateCheckTests.swift. JSON decoding is the shell's job, so that test is not ported.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use dictator_core::app_version::AppVersion;
use dictator_core::update_check::{self, ReleaseInfo};

fn release(tag: &str) -> ReleaseInfo {
    ReleaseInfo::new(tag, &format!("https://github.com/o/r/releases/tag/{tag}"))
}

fn draft(tag: &str) -> ReleaseInfo {
    ReleaseInfo {
        draft: true,
        ..release(tag)
    }
}

fn prerelease(tag: &str) -> ReleaseInfo {
    ReleaseInfo {
        prerelease: true,
        ..release(tag)
    }
}

fn v(text: &str) -> AppVersion {
    AppVersion::parse(text).expect(text)
}

fn newest_version(releases: &[ReleaseInfo], current: &str) -> Option<String> {
    update_check::newest(releases, &v(current)).map(|u| u.version.to_string())
}

#[test]
fn picks_the_newest_higher_version() {
    let releases = [
        release("v0.1.0"),
        release("v0.3.0"),
        release("v0.2.5"),
        release("nightly"),
    ];
    let update = update_check::newest(&releases, &v("0.1.0")).expect("an update");
    assert_eq!(update.version.to_string(), "0.3.0");
    assert_eq!(
        update.page_url,
        "https://github.com/o/r/releases/tag/v0.3.0"
    );
}

#[test]
fn nothing_when_up_to_date_or_ahead() {
    let releases = [release("v0.1.0"), release("v0.0.1")];
    assert_eq!(newest_version(&releases, "0.1.0"), None);
    assert_eq!(newest_version(&releases, "0.2.0"), None);
    assert_eq!(newest_version(&[], "0.1.0"), None);
}

#[test]
fn ignores_drafts() {
    let releases = [draft("v0.9.0"), release("v0.2.0")];
    assert_eq!(newest_version(&releases, "0.1.0").as_deref(), Some("0.2.0"));
}

#[test]
fn below_one_point_oh_github_prereleases_count() {
    assert_eq!(
        newest_version(&[prerelease("v0.2.0")], "0.1.0").as_deref(),
        Some("0.2.0")
    );
}

#[test]
fn stable_builds_from_one_point_oh_skip_prereleases() {
    let releases = [
        prerelease("v1.1.0"),
        release("v1.2.0-beta.1"),
        release("v1.0.1"),
    ];
    assert_eq!(newest_version(&releases, "1.0.0").as_deref(), Some("1.0.1"));
    assert_eq!(
        newest_version(&releases, "1.1.0-beta.1").as_deref(),
        Some("1.2.0-beta.1")
    );
}

#[test]
fn a_release_beats_its_own_prerelease() {
    assert_eq!(
        newest_version(&[release("v0.2.0")], "0.2.0-beta.3").as_deref(),
        Some("0.2.0")
    );
}

#[test]
fn rejects_non_https_pages() {
    let releases = [ReleaseInfo::new("v9.0.0", "http://example.com/x")];
    assert_eq!(newest_version(&releases, "0.1.0"), None);
}

#[test]
fn checks_daily() {
    let now = UNIX_EPOCH + Duration::from_secs(1_000_000);
    let hour = Duration::from_secs(3600);
    assert!(update_check::is_due(None, now));
    assert!(!update_check::is_due(Some(now - hour), now));
    assert!(update_check::is_due(
        Some(now - Duration::from_secs(86_400)),
        now
    ));
    assert!(update_check::is_due(Some(now + hour), now)); // clock went back
    let _: SystemTime = now;
}
