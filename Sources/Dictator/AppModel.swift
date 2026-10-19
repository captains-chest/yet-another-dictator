import AppKit
import DictationCore
import ModelSession
import Observation
import OSLog
import ParakeetTranscription
import WhisperTranscription

/// Composition root: builds the concrete adapters and wires them to the core controller.
@MainActor
@Observable
final class AppModel {
    let controller: DictationController
    /// Dictation and undo shortcuts (registration, storage, errors).
    let shortcuts: ShortcutSettings
    var shortcut: Shortcut { shortcuts.shortcut }
    var undoShortcut: Shortcut? { shortcuts.undoShortcut }
    var undoShortcutError: String? { shortcuts.undoShortcutError }
    var shortcutError: String? { shortcuts.shortcutError }
    var hotkeyMode: EventTapHotkey.Mode? { shortcuts.hotkeyMode }

    /// Keep the microphone running between dictations so the first syllable is never cut off.
    var keepsMicrophoneReady: Bool {
        didSet {
            defaults.set(keepsMicrophoneReady, forKey: Self.keepsMicrophoneReadyKey)
            recorder.setKeepsReady(keepsMicrophoneReady)
        }
    }

    /// Which input device dictation records from.
    let microphone: MicrophoneSettings
    @ObservationIgnored private let recorder: MicrophoneRecorder
    @ObservationIgnored private let hotkey: EventTapHotkey
    @ObservationIgnored private let whatsNew = WhatsNewWindow()
    /// Daily check, and installing a release when the user asks.
    let updates: UpdateController
    @ObservationIgnored private let overlay: OverlayPanel
    @ObservationIgnored private let defaults = UserDefaults.standard
    private static let lastSeenVersionKey = "lastSeenVersion"
    private static let keepsMicrophoneReadyKey = "keepMicrophoneReady"
    private static let languageKey = "language"
    private static let log = Logger(subsystem: "com.captainduckay.dictator", category: "state")

    /// The chosen model, the one in use, downloads and deleting (ADR 0007).
    let library: ModelLibrary
    @ObservationIgnored private let modelPicker = ModelPickerWindow()

    init() {
        let engine = ActiveTranscriber()
        let recorder = MicrophoneRecorder()
        self.recorder = recorder
        let microphone = MicrophoneSettings(source: AudioInputDeviceMonitor(), defaults: .standard)
        self.microphone = microphone
        recorder.setPreferredDevice(uid: microphone.choice?.uid)
        microphone.onChoiceChange = { uid in recorder.setPreferredDevice(uid: uid) }
        microphone.onDevicesChange = { recorder.inputDevicesChanged() }
        microphone.start()
        let controller = DictationController(
            recorder: recorder,
            transcriber: engine,
            inserter: KeystrokeTextInserter(),
            precedingText: FocusedTextReader()
        )
        self.controller = controller
        self.library = ModelLibrary.bundled(
            transport: RangedTransport(),
            defaults: .standard,
            engine: engine,
            makeTranscriber: { model, folder in
                switch model {
                case .parakeetV2: ModelSession.parakeet(modelFolder: folder)
                case .whisperLargeV3Turbo: ModelSession.whisperKit(modelFolder: folder)
                }
            },
            onActivated: { await controller.loadModel() }
        )
        let overlay = OverlayPanel(controller: controller)
        self.overlay = overlay
        self.updates = UpdateController(fetcher: UpdateChecker(), presenter: overlay)
        let hotkey = EventTapHotkey()
        self.hotkey = hotkey
        self.shortcuts = ShortcutSettings(hotkey: hotkey, defaults: .standard)
        self.keepsMicrophoneReady = defaults.bool(forKey: Self.keepsMicrophoneReadyKey)
        controller.language = DictationLanguage(storedValue: defaults.string(forKey: Self.languageKey))

        let log = Self.log
        controller.onStateChange = { [overlay] state in
            // State only — transcripts and audio are never logged.
            log.notice("state: \(String(describing: state), privacy: .public)")
            overlay.update(for: state)
        }
        recorder.onCapacityReached = { controller.recordingReachedLimit() }
        recorder.onFailure = { message in
            log.error("microphone: \(message, privacy: .public)")
            controller.recordingFailed(message)
        }
        controller.onTiming = { timing in
            // Durations only; never text or audio.
            log.notice("dictation: \(timing.audioSeconds, format: .fixed(precision: 1), privacy: .public) s audio, release→typed \(timing.releaseToTypedSeconds, format: .fixed(precision: 2), privacy: .public) s, transcription \(timing.transcriptionSeconds, format: .fixed(precision: 2), privacy: .public) s")
        }
        controller.onError = { [overlay] message in overlay.flash(message) }
        hotkey.onPress = { controller.hotkeyPressed() }
        hotkey.onRelease = { controller.hotkeyReleased() }
        hotkey.onInterrupt = { controller.cancel() }
        // Smart spacing: know when the cursor may have moved since the last dictation.
        hotkey.onUndo = { Task { await controller.undoLastDictation() } }
        // Clicks and arrow keys inside our own menu don't move the text cursor.
        hotkey.onOtherInput = { controller.noteOtherInput() }
        let center = NotificationCenter.default
        _ = center.addObserver(forName: NSMenu.didBeginTrackingNotification, object: nil, queue: .main) { [weak self] _ in
            MainActor.assumeIsolated { self?.hotkey.setOwnMenuOpen(true) }
        }
        _ = center.addObserver(forName: NSMenu.didEndTrackingNotification, object: nil, queue: .main) { [weak self] _ in
            MainActor.assumeIsolated { self?.hotkey.setOwnMenuOpen(false) }
        }
        _ = NSWorkspace.shared.notificationCenter.addObserver(
            forName: NSWorkspace.didActivateApplicationNotification, object: nil, queue: .main
        ) { _ in
            MainActor.assumeIsolated { controller.noteOtherInput() }
        }

        if ProcessInfo.processInfo.environment["DICTATOR_SETTINGS_PREVIEW"] == "1" {
            // Visual check only: show Settings without a keyboard tap, microphone, or download.
            Task { @MainActor in
                try? await Task.sleep(for: .milliseconds(400))
                NSApp.setActivationPolicy(.regular)
                NSApp.activate(ignoringOtherApps: true)
                NSApp.sendAction(Selector(("showSettingsWindow:")), to: nil, from: nil)
            }
        } else {
            Task {
                // Another copy already running owns the keyboard tap; quit before installing ours.
                guard await SingleInstance.claim() else {
                    NSApp.terminate(nil)
                    return
                }
                start()
            }
        }
    }

    private func start() {
        showWhatsNewIfUpdated()
        updates.start()
        shortcuts.start()
        if library.needsChoice { showModelPicker() }
        Task { await library.start() }
        Task {
            if Permissions.microphone == .notDetermined {
                _ = await Permissions.requestMicrophone()
            }
            recorder.prepare()
            recorder.setKeepsReady(keepsMicrophoneReady)
        }
    }

    func showModelPicker() {
        modelPicker.show(library: library) { [library] model in
            Task { await library.choose(model) }
        }
    }

    func showWhatsNew() {
        whatsNew.showCurrent()
    }

    /// Shows the release notes once, on the first launch after the version changed. A fresh
    /// install gets a short welcome with this version's notes instead.
    private func showWhatsNewIfUpdated() {
        guard let current = WhatsNewWindow.currentVersion else { return }
        let lastSeen = defaults.string(forKey: Self.lastSeenVersionKey)
        defaults.set(current.description, forKey: Self.lastSeenVersionKey)
        switch WhatsNew.decide(current: current, lastSeen: lastSeen, changelog: WhatsNewWindow.bundledChangelog()) {
        case .none:
            break
        case .welcome(let entries):
            whatsNew.show(.init(
                title: "Welcome to Dictator",
                intro: "Press your shortcut (\(shortcut.displayString)), speak, and the text is typed where your cursor is.",
                entries: entries
            ))
        case .updated(let entries):
            whatsNew.show(.init(title: "What's New in Dictator", intro: "Updated to \(current.description)", entries: entries))
        }
    }

    func setShortcut(_ new: Shortcut) { shortcuts.setShortcut(new) }
    func setUndoShortcut(_ new: Shortcut) { shortcuts.setUndoShortcut(new) }
    func clearUndoShortcut() { shortcuts.clearUndoShortcut() }

    var language: DictationLanguage {
        get { controller.language }
        set {
            controller.language = newValue
            defaults.set(newValue.rawValue, forKey: Self.languageKey)
        }
    }

    /// Unregisters the global shortcut while the user records a new one.
    func suspendHotkey() { shortcuts.suspendHotkey() }
    func resumeHotkey() { shortcuts.resumeHotkey() }
    /// Retries registration, e.g. after the user granted Input Monitoring.
    func retryHotkeyIfNeeded() { shortcuts.retryHotkeyIfNeeded() }
}
