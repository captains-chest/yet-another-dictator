//! Updates through the Tauri updater: a signed per-user installer from the GitHub release.
//! Checked at launch and then daily when allowed; installing is the user's choice in the tray.

use std::sync::Mutex;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use dictator_core::update_check;
use tauri::{AppHandle, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};

use crate::SharedApp;
use crate::app::lock;

/// The update found by the last check, if any.
#[derive(Default)]
struct Pending(Mutex<Option<Update>>);

fn pending(handle: &AppHandle) -> &Pending {
    if handle.try_state::<Pending>().is_none() {
        handle.manage(Pending::default());
    }
    handle.state::<Pending>().inner()
}

pub fn available_version(handle: &AppHandle) -> Option<String> {
    lock(&pending(handle).0)
        .as_ref()
        .map(|update| update.version.clone())
}

pub fn check_if_due(handle: AppHandle) {
    let app = handle.state::<SharedApp>();
    let settings = app.settings();
    let last = settings
        .last_update_check
        .map(|seconds| UNIX_EPOCH + Duration::from_secs(seconds));
    let now = SystemTime::now();
    if !settings.check_for_updates || !update_check::is_due(last, now) {
        return;
    }
    app.set_last_update_check(now.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs()));
    tauri::async_runtime::spawn(async move {
        let found = match handle.updater() {
            Ok(updater) => updater.check().await,
            Err(error) => Err(error),
        };
        match found {
            Ok(update) => {
                *lock(&pending(&handle).0) = update;
                crate::refresh_tray(&handle);
            }
            Err(error) => eprintln!("Update check failed: {error}"),
        }
    });
}

/// Re-checks hourly; [`check_if_due`] lets one through a day.
pub fn schedule(handle: AppHandle) {
    thread::spawn(move || {
        loop {
            thread::sleep(Duration::from_secs(60 * 60));
            check_if_due(handle.clone());
        }
    });
}

/// Downloads the verified installer and runs it; it closes Dictator and starts the new version.
pub fn install(handle: AppHandle) {
    let Some(update) = lock(&pending(&handle).0).take() else {
        return;
    };
    tauri::async_runtime::spawn(async move {
        match update.download_and_install(|_, _| {}, || {}).await {
            Ok(()) => handle.restart(),
            Err(error) => eprintln!("Update failed: {error}"),
        }
    });
}
