import AppKit
import SwiftUI

struct MenuContent: View {
    let model: AppModel
    @Environment(\.openSettings) private var openSettings

    var body: some View {
        if let download = model.library.statusText {
            Text(download)
            if model.library.canRetry {
                Button("Retry Model Download") { Task { await model.library.retry() } }
            }
            if model.library.needsChoice {
                Button("Choose Speech Model…") { model.showModelPicker() }
            }
        } else {
            Text(model.controller.state.statusText)
        }
        Text("Shortcut: \(model.shortcut.displayString)")
        if let error = model.controller.lastError {
            Text(error)
        }
        if model.controller.state == .recording || model.controller.state == .transcribing {
            Button("Cancel Dictation") { model.controller.cancel() }
        }
        Button(undoTitle) {
            // Let the menu close so the field you dictated into has keyboard focus again.
            Task {
                try? await Task.sleep(for: .milliseconds(250))
                await model.controller.undoLastDictation()
            }
        }
        .disabled(!model.controller.canUndo)
        if let text = model.controller.undeliveredTranscript {
            Divider()
            Text("Not typed: “\(Self.preview(text))”")
            Button("Type It Again") {
                // Let the menu close so the field you were in has keyboard focus again.
                Task {
                    try? await Task.sleep(for: .milliseconds(250))
                    await model.controller.retryUndelivered()
                }
            }
            Button("Discard") { model.controller.discardUndelivered() }
        }
        Divider()
        if let update = model.updates.available {
            switch model.updates.phase {
            case .idle:
                if update.canInstall {
                    Button("Install Update v\(update.version.description)…") { model.updates.install() }
                } else {
                    Button("Download Update v\(update.version.description)…") { model.updates.openReleasePage() }
                }
            case .downloading(let fraction):
                Text("Downloading update… \(Int((fraction * 100).rounded()))%")
            case .checking:
                Text("Checking update…")
            case .installing:
                Text("Installing update…")
            case .failed(let message):
                Text(message)
                Button("Try Update Again") { model.updates.install() }
            }
        }
        Button(model.updates.isChecking ? "Checking for Updates…" : "Check for Updates…") {
            model.updates.checkNow()
        }
        .disabled(model.updates.isChecking || !model.updates.phase.allowsCheck)
        Button("What's New") { model.showWhatsNew() }
        Button("Settings…") {
            NSApp.activate()
            openSettings()
        }
        .keyboardShortcut(",")
        Button("Quit Dictator") { NSApp.terminate(nil) }
            .keyboardShortcut("q")
    }

    private var undoTitle: String {
        if let shortcut = model.undoShortcut {
            "Undo Last Dictation (\(shortcut.displayString))"
        } else {
            "Undo Last Dictation"
        }
    }

    private static func preview(_ text: String) -> String {
        text.count <= 60 ? text : String(text.prefix(59)) + "…"
    }
}
