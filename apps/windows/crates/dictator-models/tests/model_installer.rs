//! Port of ModelInstallerTests.swift (manifest and installer tests), with the Windows manifest.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Mutex;

use dictator_core::StopSignal;
use dictator_models::{
    Entry, InstallError, ModelInstaller, ModelManifest, ModelTransport, ParseError, Progress,
};
use sha2::{Digest, Sha256};

const SHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn shipped_manifest() -> ModelManifest {
    ModelManifest::read(Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../model-manifest.txt"
    )))
    .unwrap()
}

mod model_manifest_tests {
    use super::*;

    #[test]
    fn parses_entries_per_model() {
        let manifest = ModelManifest::parse(&format!(
            "# comment
            small  {SHA}  10  org/repo/resolve/abc/a.bin  a.bin

            small  {SHA}  5  org/repo/resolve/abc/dir/b.bin  dir/b.bin
            dansk  {SHA}  7  org/other/resolve/def/c.bin  c.bin
            "
        ))
        .unwrap();
        assert_eq!(manifest.model_ids(), ["small", "dansk"]);
        assert_eq!(manifest.total_bytes("small"), 15);
        assert_eq!(
            manifest.files("dansk")[0].url(),
            "https://huggingface.co/org/other/resolve/def/c.bin"
        );
    }

    #[test]
    fn rejects_bad_lines() {
        for line in [
            "m  SHA  1  r/x  ../x",
            "m  SHA  1  r/x  /x",
            "m  nothex  1  r/x  x",
            "m  SHA  1  r/x",
            "m  SHA  0  r/x  x",
            "m  SHA  1  r//x  x",
            "m  SHA  1  r/x  x y",
            "m  SHA  -1  r/x  x",
            "m  SHA  1  r/x  x\\y",
        ] {
            let line = line.replace("SHA", SHA);
            assert!(
                matches!(
                    ModelManifest::parse(&line),
                    Err(ParseError::Malformed { .. })
                ),
                "{line}"
            );
        }
        assert!(ModelManifest::parse(&format!("m  {}  1  r/x  x", SHA.to_uppercase())).is_err());
    }

    #[test]
    fn rejects_duplicate_entries() {
        let line = format!("m  {SHA}  1  r/x  x\n");
        assert_eq!(
            ModelManifest::parse(&line.repeat(2)),
            Err(ParseError::Malformed {
                line: "duplicate entry".into()
            })
        );
    }

    #[test]
    fn the_shipped_manifest_parses() {
        let manifest = shipped_manifest();
        assert_eq!(
            manifest.model_ids(),
            [
                "parakeet-tdt-0.6b-v2-int8",
                "whisper-large-v3-turbo-q5_0",
                "s1-mini"
            ]
        );
        assert!(
            manifest
                .files("parakeet-tdt-0.6b-v2-int8")
                .iter()
                .any(|e| e.local_path == "tokens.txt")
        );
    }

    /// S1-mini is pinned to one immutable commit, with the hash and size of the Q4_K_M GGUF, and
    /// downloads its LICENSE and NOTICE with it (ADR 0011).
    #[test]
    fn the_shipped_manifest_pins_s1_mini() {
        let manifest = shipped_manifest();
        let files = manifest.files("s1-mini");
        let revision =
            "superwhisper/s1-mini-GGUF/resolve/34add00a48a2e5d24e5a4ee5405a99620a3a240c/";

        assert_eq!(
            files
                .iter()
                .map(|e| e.local_path.as_str())
                .collect::<Vec<_>>(),
            ["s1-mini-q4_k_m.gguf", "LICENSE", "NOTICE"]
        );
        assert!(files.iter().all(|e| e.remote_path.starts_with(revision)));
        let gguf = files[0];
        assert_eq!(
            gguf.sha256,
            "3b41ebe2502cbd03e811d5d16b022f5ab551eda58d62597d152f89535003c634"
        );
        assert_eq!(gguf.bytes, 484_219_808);
        assert_eq!(
            gguf.url(),
            format!("https://huggingface.co/{revision}s1-mini-q4_k_m.gguf")
        );
    }
}

/// Serves in-memory files, optionally failing once mid-way or corrupting a byte.
#[derive(Default)]
struct FakeTransport {
    files: BTreeMap<String, Vec<u8>>,
    fail_once_after: Mutex<Option<usize>>,
    corrupt: bool,
    resumed_from: Mutex<Vec<u64>>,
}

impl FakeTransport {
    fn new(files: &BTreeMap<String, Vec<u8>>) -> Self {
        Self {
            files: files.clone(),
            ..Self::default()
        }
    }
}

impl ModelTransport for FakeTransport {
    fn fetch(
        &self,
        entry: &Entry,
        part: &Path,
        progress: &mut dyn FnMut(u64),
        _stop: &StopSignal,
    ) -> Result<(), InstallError> {
        let mut data = self.files[&entry.local_path].clone();
        if self.corrupt {
            data[0] ^= 0xFF;
        }
        let existing = std::fs::read(part).unwrap_or_default();
        self.resumed_from
            .lock()
            .unwrap()
            .push(existing.len() as u64);
        let rest = &data[existing.len()..];
        if let Some(cut) = self.fail_once_after.lock().unwrap().take() {
            std::fs::write(part, [&existing[..], &rest[..cut.min(rest.len())]].concat())?;
            return Err(InstallError::Transport(
                "The network connection was lost.".into(),
            ));
        }
        std::fs::write(part, [&existing[..], rest].concat())?;
        progress(data.len() as u64);
        Ok(())
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

struct Fixture {
    files: BTreeMap<String, Vec<u8>>,
    root: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        let files = BTreeMap::from([
            ("a.bin".to_string(), vec![1; 1000]),
            (
                "dir/b.bin".to_string(),
                (0..300).map(|i| (i % 256) as u8).collect(),
            ),
        ]);
        Self {
            files,
            root: tempfile::tempdir().unwrap(),
        }
    }

    fn manifest(&self) -> ModelManifest {
        let lines: Vec<String> = self
            .files
            .iter()
            .map(|(path, data)| {
                format!(
                    "m  {}  {}  org/r/resolve/c/{path}  {path}",
                    hex(&Sha256::digest(data)),
                    data.len()
                )
            })
            .collect();
        ModelManifest::parse(&(lines.join("\n") + "\n")).unwrap()
    }

    fn installer(&self) -> ModelInstaller {
        let mut installer =
            ModelInstaller::new(self.manifest(), "m", self.root.path().join("Models"));
        installer.max_attempts = 1;
        installer
    }

    fn transport(&self) -> FakeTransport {
        FakeTransport::new(&self.files)
    }
}

fn install(
    installer: &ModelInstaller,
    transport: &FakeTransport,
) -> Result<Vec<Progress>, InstallError> {
    let mut reported = Vec::new();
    installer.install(transport, &mut |p| reported.push(p), &StopSignal::new())?;
    Ok(reported)
}

#[test]
fn installs_verifies_and_marks_complete() {
    let fixture = Fixture::new();
    let installer = fixture.installer();
    assert!(!installer.is_installed());
    let reported = install(&installer, &fixture.transport()).unwrap();
    assert!(installer.is_installed());
    assert_eq!(
        std::fs::read(installer.folder().join("dir/b.bin")).unwrap(),
        fixture.files["dir/b.bin"]
    );
    assert_eq!(
        reported.last(),
        Some(&Progress {
            completed_bytes: 1300,
            total_bytes: 1300
        })
    );
    assert!(
        reported
            .windows(2)
            .all(|p| p[0].completed_bytes <= p[1].completed_bytes)
    );
}

#[test]
fn resumes_after_a_dropped_connection() {
    let fixture = Fixture::new();
    let installer = fixture.installer();
    let transport = fixture.transport();
    *transport.fail_once_after.lock().unwrap() = Some(400);
    assert!(matches!(
        install(&installer, &transport),
        Err(InstallError::Transport(_))
    ));
    assert!(!installer.is_installed());
    install(&installer, &transport).unwrap();
    assert!(installer.is_installed());
    assert!(transport.resumed_from.lock().unwrap().contains(&400));
}

#[test]
fn rejects_a_corrupt_download() {
    let fixture = Fixture::new();
    let installer = fixture.installer();
    let transport = FakeTransport {
        corrupt: true,
        ..fixture.transport()
    };
    assert_eq!(
        install(&installer, &transport),
        Err(InstallError::ChecksumMismatch("a.bin".into()))
    );
    assert_eq!(
        InstallError::ChecksumMismatch("a.bin".into()).to_string(),
        "Downloaded file failed verification: a.bin"
    );
    assert!(!installer.is_installed());
    assert!(!installer.folder().join("a.bin").exists());
}

#[test]
fn removes_models_no_longer_in_the_manifest() {
    let fixture = Fixture::new();
    let installer = fixture.installer();
    let stale = installer.root.join("old-model");
    std::fs::create_dir_all(&stale).unwrap();
    install(&installer, &fixture.transport()).unwrap();
    assert!(!stale.exists());
    assert!(installer.folder().exists());
}

#[test]
fn unknown_model_fails() {
    let fixture = Fixture::new();
    let installer = ModelInstaller::new(fixture.manifest(), "nope", fixture.root.path());
    let error = install(&installer, &fixture.transport()).unwrap_err();
    assert_eq!(error.to_string(), "Model nope is not in the manifest");
}

#[test]
fn an_installed_model_is_not_downloaded_again() {
    let fixture = Fixture::new();
    let installer = fixture.installer();
    install(&installer, &fixture.transport()).unwrap();
    let transport = fixture.transport();
    install(&installer, &transport).unwrap();
    assert!(transport.resumed_from.lock().unwrap().is_empty());
}

#[test]
fn a_manifest_bump_means_reinstalling() {
    let mut fixture = Fixture::new();
    let installer = fixture.installer();
    install(&installer, &fixture.transport()).unwrap();
    fixture.files.insert("a.bin".into(), vec![2; 1000]);
    let bumped = ModelInstaller {
        manifest: fixture.manifest(),
        ..installer.clone()
    };
    assert!(!bumped.is_installed());
    let transport = fixture.transport();
    install(&bumped, &transport).unwrap();
    assert_eq!(
        *transport.resumed_from.lock().unwrap(),
        [0],
        "only the changed file is fetched"
    );
}

#[test]
fn cancellation_stops_the_install() {
    let fixture = Fixture::new();
    let installer = fixture.installer();
    let stop = StopSignal::new();
    stop.set();
    let result = installer.install(&fixture.transport(), &mut |_| {}, &stop);
    assert_eq!(result, Err(InstallError::Cancelled));
    assert!(!installer.is_installed());
}

#[test]
fn uninstall_deletes_the_folder() {
    let fixture = Fixture::new();
    let installer = fixture.installer();
    installer.uninstall().unwrap(); // nothing there yet
    install(&installer, &fixture.transport()).unwrap();
    installer.uninstall().unwrap();
    assert!(!installer.folder().exists());
    assert!(!installer.is_installed());
}

#[test]
fn default_root_is_under_local_app_data() {
    use dictator_models::installer::default_root_from;
    use std::ffi::OsString;
    use std::path::PathBuf;

    let env = |vars: &'static [(&'static str, &'static str)]| {
        move |key: &str| {
            vars.iter()
                .find(|(k, _)| *k == key)
                .map(|(_, v)| OsString::from(v))
        }
    };
    let models = |base: &str| Some(PathBuf::from(base).join("Dictator").join("Models"));
    assert_eq!(
        default_root_from(env(&[("LOCALAPPDATA", "L"), ("USERPROFILE", "U")])),
        models("L")
    );
    assert_eq!(
        default_root_from(env(&[("USERPROFILE", "U")])),
        models("U/AppData/Local")
    );
    assert_eq!(
        default_root_from(env(&[("HOME", "H")])),
        models("H/.local/share")
    );
    assert_eq!(default_root_from(env(&[])), None);
}
