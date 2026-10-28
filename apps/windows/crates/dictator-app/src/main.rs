//! Dictator for Windows: a tray app. Hold the shortcut, speak, and the text is typed where the
//! cursor is. Everything runs on this PC.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod dictation;
mod settings;
mod updates;

use std::sync::{Arc, Mutex, mpsc};
use std::thread;

use dictator_core::app_version::AppVersion;
use dictator_core::dictation_controller::{DictationState, Event};
use dictator_core::whats_new::{self, ChangelogEntry, Decision};
use dictator_models::manifest::{self, ModelManifest};
use dictator_windows::hook::Verdict;
use dictator_windows::hotkey::HotkeyAction;
use dictator_windows::platform;
use tauri::menu::{Menu, MenuBuilder, MenuItem, PredefinedMenuItem};
use tauri::path::BaseDirectory;
use tauri::tray::{TrayIcon, TrayIconBuilder};
use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, RunEvent, State, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder, WindowEvent, Wry,
};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};

use crate::app::{App, Shell, ShortcutKind, View, lock};
use crate::dictation::Snapshot;

const TRAY_ID: &str = "dictator";
const OVERLAY: &str = "overlay";

type SharedApp = Arc<App>;

// MARK: Shell

struct TauriShell {
    handle: AppHandle,
}

impl Shell for TauriShell {
    fn view_changed(&self, view: &View) {
        let _ = self.handle.emit("view-changed", view);
    }

    fn dictation_changed(&self, snapshot: &Snapshot, shortcut: &str) {
        update_overlay(&self.handle, &snapshot.state);
        update_tray(&self.handle, snapshot, shortcut);
    }

    fn level(&self, level: f32) {
        let _ = self.handle.emit_to(OVERLAY, "level", level);
    }

    fn set_open_at_login(&self, on: bool) -> Result<(), String> {
        let launcher = self.handle.autolaunch();
        if on {
            launcher.enable()
        } else {
            launcher.disable()
        }
        .map_err(|error| error.to_string())
    }

    fn open_at_login(&self) -> bool {
        self.handle.autolaunch().is_enabled().unwrap_or(false)
    }
}

// MARK: Tray

fn status_text(snapshot: &Snapshot, shortcut: &str) -> String {
    match &snapshot.state {
        DictationState::LoadingModel => "Loading the speech model…".into(),
        DictationState::Ready => format!("Ready: hold {shortcut} and speak"),
        DictationState::Recording => "Listening…".into(),
        DictationState::Transcribing => "Transcribing…".into(),
        DictationState::Unavailable(reason) => reason.clone(),
    }
}

fn tray_menu(handle: &AppHandle, snapshot: &Snapshot, shortcut: &str) -> tauri::Result<Menu<Wry>> {
    let item = |id: &str, text: &str, enabled: bool| {
        MenuItem::with_id(handle, id, text, enabled, None::<&str>)
    };
    let busy = matches!(
        snapshot.state,
        DictationState::Recording | DictationState::Transcribing
    );
    let mut menu =
        MenuBuilder::new(handle).item(&item("status", &status_text(snapshot, shortcut), false)?);
    if let Some(error) = &snapshot.error {
        menu = menu.item(&item("error", error, false)?);
    }
    menu = menu
        .item(&PredefinedMenuItem::separator(handle)?)
        .item(&item("cancel", "Cancel", busy)?)
        .item(&item("undo", "Undo Last Dictation", snapshot.can_undo)?);
    if snapshot.undelivered {
        menu = menu
            .item(&PredefinedMenuItem::separator(handle)?)
            .item(&item(
                "undelivered",
                "Your last dictation wasn't typed",
                false,
            )?)
            .item(&item("retry", "Type It Again", true)?)
            .item(&item("discard", "Discard", true)?);
    }
    menu = menu.item(&PredefinedMenuItem::separator(handle)?);
    if let Some(version) = updates::available_version(handle) {
        menu = menu.item(&item(
            "install-update",
            &format!("Install Update {version}"),
            true,
        )?);
    }
    menu.item(&item("whats-new", "What's New", true)?)
        .item(&item("settings", "Settings…", true)?)
        .item(&PredefinedMenuItem::separator(handle)?)
        .item(&item("quit", "Quit Dictator", true)?)
        .build()
}

fn update_tray(handle: &AppHandle, snapshot: &Snapshot, shortcut: &str) {
    let Some(tray) = handle.tray_by_id(TRAY_ID) else {
        return;
    };
    let tooltip = format!("Dictator: {}", status_text(snapshot, shortcut));
    let _ = tray.set_tooltip(Some(&tooltip));
    match tray_menu(handle, snapshot, shortcut) {
        Ok(menu) => {
            let _ = tray.set_menu(Some(menu));
        }
        Err(error) => eprintln!("Couldn't build the tray menu: {error}"),
    }
}

/// Re-renders the tray, e.g. after an update was found.
pub fn refresh_tray(handle: &AppHandle) {
    let app = handle.state::<SharedApp>();
    if let Some(snapshot) = app.snapshot() {
        update_tray(
            handle,
            &snapshot,
            &app.settings().shortcut().display_string(),
        );
    }
}

fn create_tray(handle: &AppHandle) -> tauri::Result<TrayIcon> {
    let starting = Snapshot {
        state: DictationState::LoadingModel,
        error: None,
        undelivered: false,
        can_undo: false,
    };
    let mut tray = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("Dictator")
        .menu(&tray_menu(handle, &starting, "")?)
        .show_menu_on_left_click(true)
        .on_menu_event(|handle, event| on_menu(handle, event.id().as_ref()));
    if let Some(icon) = handle.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(handle)
}

fn on_menu(handle: &AppHandle, id: &str) {
    let app = handle.state::<SharedApp>();
    match id {
        "cancel" => app.runner.send(Event::Cancel),
        "undo" => app.runner.send(Event::UndoRequested),
        "retry" => app.retry_undelivered(),
        "discard" => app.runner.send(Event::DiscardUndelivered),
        "install-update" => updates::install(handle.clone()),
        "whats-new" => open_window(handle, "whats-new"),
        "settings" => open_window(handle, "settings"),
        "quit" => handle.exit(0),
        _ => {}
    }
}

// MARK: Windows

fn open_window(handle: &AppHandle, label: &str) {
    if let Some(window) = handle.get_webview_window(label) {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
        return;
    }
    let (title, width, height) = match label {
        "picker" => ("Welcome to Dictator", 520.0, 520.0),
        "whats-new" => ("What's New in Dictator", 520.0, 480.0),
        _ => ("Dictator Settings", 600.0, 720.0),
    };
    let built = WebviewWindowBuilder::new(
        handle,
        label,
        WebviewUrl::App(format!("{label}.html").into()),
    )
    .title(title)
    .inner_size(width, height)
    .min_inner_size(420.0, 360.0)
    .center()
    .build();
    if let Err(error) = built {
        eprintln!("Couldn't open {label}: {error}");
    }
}

/// A small pill at the bottom of the screen while listening or transcribing. It never takes focus
/// or clicks, so typing goes to the app being dictated into.
fn create_overlay(handle: &AppHandle) -> tauri::Result<WebviewWindow> {
    WebviewWindowBuilder::new(handle, OVERLAY, WebviewUrl::App("overlay.html".into()))
        .title("Dictator")
        .inner_size(180.0, 44.0)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .focused(false)
        .focusable(false)
        .visible(false)
        .build()
}

fn update_overlay(handle: &AppHandle, state: &DictationState) {
    let Some(overlay) = handle.get_webview_window(OVERLAY) else {
        return;
    };
    let phase = match state {
        DictationState::Recording => "listening",
        DictationState::Transcribing => "transcribing",
        _ => {
            let _ = overlay.hide();
            return;
        }
    };
    let _ = overlay.emit_to(OVERLAY, "overlay", phase);
    if overlay.is_visible().unwrap_or(false) {
        return;
    }
    if let (Ok(Some(monitor)), Ok(size)) = (
        overlay
            .current_monitor()
            .or_else(|_| overlay.primary_monitor()),
        overlay.outer_size(),
    ) {
        let area = monitor.work_area();
        let margin = (24.0 * monitor.scale_factor()) as i32;
        let x = area.position.x + (area.size.width as i32 - size.width as i32) / 2;
        let y = area.position.y + area.size.height as i32 - size.height as i32 - margin;
        let _ = overlay.set_position(PhysicalPosition::new(x, y));
    }
    let _ = overlay.set_ignore_cursor_events(true);
    let _ = overlay.show();
}

// MARK: Commands

#[tauri::command]
fn view(app: State<'_, SharedApp>) -> View {
    app.refresh_microphones();
    app.view()
}

#[tauri::command]
fn select_model(app: State<'_, SharedApp>, id: String) {
    app.select_model(&id);
}

#[tauri::command]
fn delete_model(app: State<'_, SharedApp>, id: String) {
    app.delete_model(&id);
}

#[tauri::command]
fn choose_first_model(handle: AppHandle, app: State<'_, SharedApp>, id: String) {
    app.select_model(&id);
    if let Some(picker) = handle.get_webview_window("picker") {
        let _ = picker.close();
    }
    open_window(&handle, "settings");
}

#[tauri::command]
fn record_shortcut(app: State<'_, SharedApp>, kind: ShortcutKind) {
    app.record_shortcut(kind);
}

#[tauri::command]
fn clear_undo_shortcut(app: State<'_, SharedApp>) {
    app.clear_undo_shortcut();
}

#[tauri::command]
fn set_language(app: State<'_, SharedApp>, code: Option<String>) {
    app.set_language(code.as_deref());
}

#[tauri::command]
fn set_cleanup_enabled(app: State<'_, SharedApp>, enabled: bool) {
    app.set_cleanup_enabled(enabled);
}

#[tauri::command]
fn set_cleanup_styling(app: State<'_, SharedApp>, styling: String) {
    app.set_cleanup_styling(&styling);
}

#[tauri::command]
fn delete_cleanup_model(app: State<'_, SharedApp>) {
    app.delete_cleanup_model();
}

#[tauri::command]
fn set_microphone(app: State<'_, SharedApp>, id: Option<String>) {
    app.set_microphone(id.as_deref());
}

#[tauri::command]
fn set_keep_microphone_ready(app: State<'_, SharedApp>, on: bool) {
    app.set_keep_microphone_ready(on);
}

#[tauri::command]
fn set_open_at_login(app: State<'_, SharedApp>, on: bool) {
    app.set_open_at_login(on);
}

#[tauri::command]
fn set_check_for_updates(handle: AppHandle, app: State<'_, SharedApp>, on: bool) {
    app.set_check_for_updates(on);
    if on {
        updates::check_if_due(handle);
    }
}

#[tauri::command]
fn whats_new(state: State<'_, WhatsNew>) -> String {
    lock(&state.0).clone()
}

// MARK: What's New

/// The text the What's New window shows.
struct WhatsNew(Mutex<String>);

fn format_notes(entries: &[ChangelogEntry]) -> String {
    entries
        .iter()
        .map(|entry| format!("{}: {}\n\n{}", entry.version, entry.note, entry.body.trim()))
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// Shows the notes after an update (or a welcome on first launch) and remembers this version.
fn whats_new_at_launch(handle: &AppHandle, app: &App) {
    let changelog = handle
        .path()
        .resolve("CHANGELOG.md", BaseDirectory::Resource)
        .ok()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .map(|text| whats_new::parse_changelog(&text))
        .unwrap_or_default();
    let Some(current) = AppVersion::parse(&app.version) else {
        return;
    };
    let decision = whats_new::decide(
        &current,
        app.settings().last_seen_version.as_deref(),
        &changelog,
    );
    let latest = changelog
        .iter()
        .max_by(|a, b| a.version.cmp(&b.version))
        .cloned();
    let (notes, show) = match decision {
        Decision::Updated(entries) if !entries.is_empty() => (format_notes(&entries), true),
        Decision::Welcome(entries) => (format_notes(&entries), false),
        _ => (format_notes(&latest.into_iter().collect::<Vec<_>>()), false),
    };
    *lock(&handle.state::<WhatsNew>().0) = if notes.is_empty() {
        "No release notes.".into()
    } else {
        notes
    };
    app.set_last_seen_version(&app.version);
    if show {
        open_window(handle, "whats-new");
    }
}

// MARK: Keyboard

/// Watches the keyboard on the hook thread; decisions there must be instant, so the resulting
/// actions are handled on their own thread.
fn start_hotkeys(app: &SharedApp) -> Result<platform::HookGuard, String> {
    let (sender, receiver) = mpsc::channel::<HotkeyAction>();
    let worker = app.clone();
    thread::Builder::new()
        .name("dictator-hotkeys".into())
        .spawn(move || {
            for action in receiver {
                worker.hotkey_action(action);
            }
        })
        .map_err(|error| error.to_string())?;
    let listener = app.hotkeys();
    platform::install_hook(move |input| {
        let outcome = lock(&listener).handle(input);
        for action in outcome.actions {
            let _ = sender.send(action);
        }
        Verdict {
            swallow: outcome.swallow,
            mask_menu: outcome.mask_menu,
        }
    })
}

// MARK: Main

fn bundled_manifest(handle: &AppHandle) -> Option<ModelManifest> {
    let path = handle
        .path()
        .resolve(manifest::FILE_NAME, BaseDirectory::Resource)
        .ok()?;
    ModelManifest::read(&path)
        .or_else(|_| ModelManifest::bundled())
        .ok()
}

fn setup(handle: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    handle.manage(WhatsNew(Mutex::new(String::new())));
    let shell = Arc::new(TauriShell {
        handle: handle.clone(),
    });
    let app = App::new(
        shell,
        bundled_manifest(handle),
        handle.package_info().version.to_string(),
    );
    handle.manage(app.clone());

    create_tray(handle)?;
    create_overlay(handle)?;
    match start_hotkeys(&app) {
        // Held by the app, so the hooks live as long as it does.
        Ok(guard) => {
            handle.manage(guard);
        }
        Err(error) => eprintln!("{error}"),
    }
    if app.start() {
        open_window(handle, "picker");
    }
    whats_new_at_launch(handle, &app);
    updates::check_if_due(handle.clone());
    updates::schedule(handle.clone());
    Ok(())
}

fn main() {
    tauri::Builder::default()
        // First, so a second launch only opens Settings in the running app.
        .plugin(tauri_plugin_single_instance::init(|handle, _, _| {
            open_window(handle, "settings")
        }))
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            view,
            select_model,
            delete_model,
            choose_first_model,
            record_shortcut,
            clear_undo_shortcut,
            set_language,
            set_cleanup_enabled,
            set_cleanup_styling,
            delete_cleanup_model,
            set_microphone,
            set_keep_microphone_ready,
            set_open_at_login,
            set_check_for_updates,
            whats_new,
        ])
        .on_window_event(|window, event| {
            if window.label() == "settings" && matches!(event, WindowEvent::Destroyed) {
                window.state::<SharedApp>().cancel_recording();
            }
        })
        .setup(|app| setup(app.handle()))
        .build(tauri::generate_context!())
        .expect("Dictator starts")
        .run(|_, event| {
            // Closing the last window keeps the tray app running; only Quit exits.
            if let RunEvent::ExitRequested {
                code: None, api, ..
            } = event
            {
                api.prevent_exit();
            }
        });
}
