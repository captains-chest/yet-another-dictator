import DictationCore
import SwiftUI

@main
struct DictatorApp: App {
    @State private var model = AppModel()

    var body: some Scene {
        MenuBarExtra {
            MenuContent(model: model)
        } label: {
            Image(systemName: model.controller.state.menuBarSymbol)
        }

        Settings {
            SettingsView(model: model)
        }
    }
}

extension DictationState {
    var menuBarSymbol: String {
        switch self {
        case .loadingModel: "hourglass"
        case .ready: "waveform"
        case .recording: "mic.fill"
        case .transcribing: "ellipsis.bubble"
        case .unavailable: "exclamationmark.triangle"
        }
    }

    var statusText: String {
        switch self {
        case .loadingModel: "Loading model… (first launch can take a few minutes)"
        case .ready: "Ready"
        case .recording: "Listening…"
        case .transcribing: "Transcribing…"
        case .unavailable(let reason): "Unavailable: \(reason)"
        }
    }
}
