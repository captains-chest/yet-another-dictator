import AppKit
import DictationCore
import SwiftUI

struct SettingsView: View {
    let model: AppModel
    @State private var microphone = Permissions.microphone
    @State private var canType = Permissions.canPostEvents
    @State private var canListen = Permissions.canListenEvents
    @State private var requestedTyping = false
    @State private var requestedListening = false
    @State private var loginItem = LoginItem.status
    @State private var loginItemError: String?
    @State private var category: SettingsCategory? = .shortcut

    var body: some View {
        HStack(spacing: 0) {
            categoryList
            Divider()
            categoryDetail
        }
        .frame(width: SettingsWindowSize.width, height: SettingsWindowSize.height)
        // Show in the Dock and ⌘Tab while Settings is open so the window can be found again.
        .onAppear {
            NSApp.setActivationPolicy(.regular)
            NSApp.activate()
        }
        .onDisappear { NSApp.setActivationPolicy(.accessory) }
        // Poll: toggling a permission in System Settings posts no notification, and a menu bar
        // app does not reliably become active again when the user returns to this window.
        // (Accessibility and Input Monitoring may be cached per process until a relaunch.)
        .onReceive(Timer.publish(every: 1, on: .main, in: .common).autoconnect()) { _ in
            refresh()
        }
        .onReceive(NotificationCenter.default.publisher(for: NSApplication.didBecomeActiveNotification)) { _ in
            refresh()
        }
        .onChange(of: category) {
            if category == nil { category = .shortcut }
        }
    }

    private var categoryList: some View {
        List(SettingsCategory.allCases, selection: $category) { item in
            Text(item.title)
        }
        .listStyle(.sidebar)
        .frame(width: SettingsWindowSize.sidebarWidth)
        .frame(maxHeight: .infinity)
        .accessibilityLabel("Settings categories")
    }

    /// The selected page. A bounded scroll view, so a tall page (the keyboard, or the permission
    /// guidance) scrolls inside the pane instead of growing the window.
    private var categoryDetail: some View {
        ScrollView {
            detailForm
                .frame(maxWidth: .infinity, alignment: .top)
        }
        .scrollBounceBehavior(.basedOnSize)
        .id(category)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    private var detailForm: some View {
        Form {
            switch category ?? .shortcut {
            case .shortcut: ShortcutSection(model: model)
            case .general: generalSection
            case .permissions: permissionsSection
            case .model: modelSection
            }
        }
        .formStyle(.grouped)
    }

    private var generalSection: some View {
        Section("General") {
            Toggle("Open at login", isOn: Binding(
                get: { loginItem != .disabled },
                set: { enabled in
                    loginItemError = LoginItem.set(enabled: enabled)
                    loginItem = LoginItem.status
                }
            ))
            if loginItem == .needsApproval {
                LabeledContent("Allow Dictator in Login Items to finish.") {
                    Button("Open Login Items…") { LoginItem.openSystemSettings() }
                }
                .foregroundStyle(.secondary)
            }
            if let loginItemError {
                Text(loginItemError).foregroundStyle(.red)
            }
            Toggle(isOn: Binding(
                get: { model.updates.checksForUpdates },
                set: { model.updates.checksForUpdates = $0 }
            )) {
                Text("Check for updates daily")
                Text("Asks GitHub whether a newer version exists. Nothing is downloaded until you choose Install Update. Dictation never uses the network.")
            }
            microphonePicker
            Toggle(isOn: Binding(
                get: { model.keepsMicrophoneReady },
                set: { model.keepsMicrophoneReady = $0 }
            )) {
                Text("Keep microphone ready")
                Text("Catches the first word even if you start speaking as you press the shortcut. macOS shows the microphone indicator while this is on. Audio is held in memory for under half a second and never saved.")
            }
        }
    }

    private var microphonePicker: some View {
        let microphone = model.microphone
        return Picker(selection: Binding(
            get: { microphone.choice?.uid },
            set: { uid in
                // Re-selecting the unplugged choice keeps it rather than switching to the default.
                guard uid != microphone.choice?.uid else { return }
                microphone.select(microphone.devices.first { $0.uid == uid })
            }
        )) {
            Text(microphone.systemDefault.map { "System Default (\($0.name))" } ?? "System Default")
                .tag(String?.none)
            ForEach(microphone.devices) { device in
                Text(device.name).tag(Optional(device.uid))
            }
            if microphone.isChoiceMissing, let choice = microphone.choice {
                Text("\(choice.name) (not connected)").tag(Optional(choice.uid))
            }
        } label: {
            Text("Microphone")
            if microphone.isChoiceMissing {
                Text("Not connected. Dictation uses \(microphone.systemDefault?.name ?? "the system default input") until it is.")
            } else {
                Text("Used only for dictation. Calls and other apps keep the input set in System Settings → Sound.")
            }
        }
    }

    private var permissionsSection: some View {
        Section("Permissions") {
            PermissionRow(title: "Microphone", granted: microphone == .granted) {
                Task {
                    if microphone == .notDetermined { _ = await Permissions.requestMicrophone() }
                    else { Permissions.openPrivacySettings("Privacy_Microphone") }
                    refresh()
                }
            }
            PermissionRow(
                title: "Input Monitoring (detect shortcut)",
                granted: canListen,
                actionTitle: requestedListening ? "Relaunch" : "Grant…"
            ) {
                if requestedListening { return Permissions.relaunch() }
                if !Permissions.requestListenEvents() { Permissions.openPrivacySettings("Privacy_ListenEvent") }
                requestedListening = true
                refresh()
            }
            PermissionRow(
                title: "Accessibility (type into fields)",
                granted: canType,
                actionTitle: requestedTyping ? "Relaunch" : "Grant…"
            ) {
                if requestedTyping { return Permissions.relaunch() }
                if !Permissions.requestPostEvents() { Permissions.openPrivacySettings("Privacy_Accessibility") }
                requestedTyping = true
                refresh()
            }
            if (requestedListening && !canListen) || (requestedTyping && !canType) {
                Text("After enabling Dictator in System Settings, relaunch to apply.")
                    .foregroundStyle(.secondary)
            }
            if !canListen || !canType {
                // An older build's entry (e.g. the unsigned 0.1.0) can stay switched on in
                // System Settings without applying to this build, and macOS won't ask again.
                VStack(alignment: .leading, spacing: 6) {
                    Text("Already switched on in System Settings? macOS may be keeping the permission for an older copy of Dictator. Select Dictator in the list, remove it with −, then click Grant… here again.")
                        .foregroundStyle(.secondary)
                    HStack {
                        if !canListen {
                            Button("Open Input Monitoring…") { Permissions.openPrivacySettings("Privacy_ListenEvent") }
                        }
                        if !canType {
                            Button("Open Accessibility…") { Permissions.openPrivacySettings("Privacy_Accessibility") }
                        }
                    }
                }
            }
        }
    }

    private var modelSection: some View {
        Section("Model") {
            LabeledContent(
                model.library.selected?.displayName ?? "Speech model",
                value: model.library.statusText ?? model.controller.state.statusText
            )
            ForEach(SpeechModel.ranked) { speech in
                HStack(alignment: .top) {
                    ModelSummary(model: speech, bytes: model.library.bytes(of: speech), recommended: speech == SpeechModel.ranked.first)
                    Spacer()
                    VStack(alignment: .trailing) {
                        if model.library.selected == speech {
                            Text("In use").foregroundStyle(.secondary)
                        } else {
                            Button(model.library.isInstalled(speech) ? "Use" : "Download and Use") {
                                Task { await model.library.choose(speech) }
                            }
                            if model.library.canDelete(speech) {
                                Button("Delete") { try? model.library.delete(speech) }
                            }
                        }
                    }
                }
            }
            if model.library.canRetry {
                Button("Retry Model Download") { Task { await model.library.retry() } }
            }
            Picker("Language", selection: Binding(get: { model.language }, set: { model.language = $0 })) {
                ForEach(DictationLanguage.allCases, id: \.self) { language in
                    Text(language.displayName).tag(language)
                }
            }
            .disabled(model.library.selected?.isEnglishOnly == true)
            if model.library.selected?.isEnglishOnly == true {
                Text("Parakeet understands English only. Switch to Whisper to choose Automatic or English.")
                    .foregroundStyle(.secondary)
            }
            Text("Runs entirely on this Mac. Audio never leaves the device and is not saved. Downloading the model you choose (from Hugging Face, verified by SHA-256) is the only network use besides the optional update check and, when you install one, the update itself.")
                .foregroundStyle(.secondary)
        }
    }

    private func refresh() {
        microphone = Permissions.microphone
        canType = Permissions.canPostEvents
        canListen = Permissions.canListenEvents
        loginItem = LoginItem.status
        model.retryHotkeyIfNeeded()
    }
}

private enum SettingsCategory: String, CaseIterable, Identifiable, Hashable {
    case shortcut
    case general
    case permissions
    case model

    var id: Self { self }

    var title: String {
        switch self {
        case .shortcut: "Shortcut"
        case .general: "General"
        case .permissions: "Permissions"
        case .model: "Model"
        }
    }
}

@MainActor
private enum SettingsWindowSize {
    static let width: CGFloat = 640
    static let sidebarWidth: CGFloat = 168
    /// Preferred content height: enough for the shortcut page, still inside a laptop display.
    static let preferredHeight: CGFloat = 520
    /// Kept clear so the title bar is not clipped by the menu bar or Dock.
    static let screenMargin: CGFloat = 28

    static var height: CGFloat {
        let visible = NSScreen.main?.visibleFrame.height ?? (preferredHeight + screenMargin)
        return min(preferredHeight, visible - screenMargin)
    }
}

private struct PermissionRow: View {
    let title: String
    let granted: Bool
    var actionTitle = "Grant…"
    let grant: () -> Void

    var body: some View {
        LabeledContent(title) {
            if granted {
                Label("Granted", systemImage: "checkmark.circle.fill").foregroundStyle(.green)
            } else {
                Button(actionTitle, action: grant)
            }
        }
    }
}

/// Shows the shortcut on a keyboard. Click the button, then hold any keys and let go to record;
/// the keyboard lights up the keys as they are pressed.
private struct ShortcutSection: View {
    let model: AppModel
    @State private var monitor: Any?
    @State private var recorder = ChordRecorder()
    @State private var target = Target.dictate

    private enum Target { case dictate, undo }

    var body: some View {
        Section("Shortcut") {
            LabeledContent("Dictate") {
                Button(isRecording(.dictate) ? "Press keys… (click to cancel)" : model.shortcut.displayString) {
                    monitor == nil ? start(.dictate) : stop()
                }
                .monospaced()
            }
            LabeledContent("Undo last dictation") {
                HStack {
                    Button(isRecording(.undo) ? "Press keys… (click to cancel)" : model.undoShortcut?.displayString ?? "None") {
                        monitor == nil ? start(.undo) : stop()
                    }
                    .monospaced()
                    if model.undoShortcut != nil, monitor == nil {
                        Button("Clear") { model.clearUndoShortcut() }
                    }
                }
            }
            if let error = model.undoShortcutError {
                Text(error).foregroundStyle(.red)
            }
            KeyboardView(highlighted: monitor == nil ? model.shortcut.codes : recorder.held)
                .frame(maxWidth: .infinity)
            if let error = model.shortcutError {
                Text(error).foregroundStyle(.red)
            } else if monitor == nil, let note = passThroughNote {
                if note.isWarning {
                    Label(note.text, systemImage: "exclamationmark.triangle.fill").foregroundStyle(.orange)
                } else {
                    Text(note.text).foregroundStyle(.secondary)
                }
            }
            Text("Any key or combination works, including fn, Caps Lock and a single modifier like Right ⌥. Tap to start and stop. Hold to talk, release to finish.")
                .foregroundStyle(.secondary)
        }
        .onDisappear { stop() }
        .onReceive(NotificationCenter.default.publisher(for: NSApplication.didResignActiveNotification)) { _ in
            stop()
        }
    }

    /// Only relevant when macOS gave us a listen-only tap, so the shortcut's keys can't be withheld.
    private var passThroughNote: (text: String, isWarning: Bool)? {
        guard model.hotkeyMode == .listenOnly else { return nil }
        let name = model.shortcut.displayString
        return switch model.shortcut.passThroughEffect {
        case .harmless: nil
        case .appShortcut:
            ("macOS only lets Dictator watch the keyboard, so \(name) also reaches the app you're in.", false)
        case .typing:
            ("macOS only lets Dictator watch the keyboard, so \(name) will also type or act in the app you're dictating into. Add ⌃ or ⌘, or use a single modifier like Right ⌥, fn or an F-key.", true)
        }
    }

    private func isRecording(_ which: Target) -> Bool {
        monitor != nil && target == which
    }

    private func start(_ which: Target) {
        target = which
        model.suspendHotkey()
        recorder = ChordRecorder()
        monitor = NSEvent.addLocalMonitorForEvents(matching: [.keyDown, .keyUp, .flagsChanged]) { event in
            guard let keyEvent = KeyEvent(event) else { return event }
            if let codes = recorder.apply(keyEvent) {
                let keys = codes.map {
                    Shortcut.Key(code: $0, label: KeyLabels.label(forKeyCode: $0, typed: KeyboardLayout.character(for: $0)))
                }
                switch target {
                case .dictate: model.setShortcut(Shortcut(keys: keys))
                case .undo: model.setUndoShortcut(Shortcut(keys: keys))
                }
                stop(resume: false)
            }
            return nil
        }
    }

    private func stop(resume: Bool = true) {
        guard let monitor else { return }
        NSEvent.removeMonitor(monitor)
        self.monitor = nil
        recorder = ChordRecorder()
        if resume { model.resumeHotkey() }
    }
}

private extension KeyEvent {
    init?(_ event: NSEvent) {
        switch event.type {
        case .keyDown: self = .down(event.keyCode)
        case .keyUp: self = .up(event.keyCode)
        case .flagsChanged: self = .flagsChanged(event.keyCode, flags: UInt64(event.modifierFlags.rawValue))
        default: return nil
        }
    }
}
