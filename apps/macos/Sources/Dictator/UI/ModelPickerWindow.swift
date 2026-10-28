import AppKit
import SwiftUI

/// Shown once at launch while no speech model has been chosen. Lists the models best first; only
/// the one the user picks is downloaded.
@MainActor
final class ModelPickerWindow {
    private var window: NSWindow?

    func show(library: ModelLibrary, onChoose: @escaping (SpeechModel) -> Void) {
        let view = ModelPickerView(library: library) { [weak self] model in
            self?.window?.close()
            onChoose(model)
        }
        if let window {
            window.contentViewController = NSHostingController(rootView: view)
        } else {
            let window = NSWindow(contentViewController: NSHostingController(rootView: view))
            window.styleMask = [.titled, .closable]
            window.isReleasedWhenClosed = false
            window.title = "Choose a Speech Model"
            window.center()
            self.window = window
        }
        NSApp.activate()
        window?.makeKeyAndOrderFront(nil)
    }
}

private struct ModelPickerView: View {
    let library: ModelLibrary
    let choose: (SpeechModel) -> Void
    @State private var selection: SpeechModel = SpeechModel.ranked[0]

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("Choose a speech model").font(.title2.bold())
            Text("Dictator transcribes on this Mac. Pick one model to download; you can switch later in Settings → Model. Nothing is downloaded until you choose.")
                .foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)
            VStack(spacing: 8) {
                ForEach(Array(SpeechModel.ranked.enumerated()), id: \.element) { index, model in
                    Button { selection = model } label: {
                        ModelSummary(model: model, bytes: library.bytes(of: model), recommended: index == 0)
                            .padding(12)
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .background(
                                RoundedRectangle(cornerRadius: 8)
                                    .strokeBorder(selection == model ? Color.accentColor : Color.secondary.opacity(0.3), lineWidth: selection == model ? 2 : 1)
                            )
                            .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                }
            }
            HStack {
                Spacer()
                Button("Download \(selection.displayName) (\(ModelLibrary.size(library.bytes(of: selection))))") {
                    choose(selection)
                }
                .keyboardShortcut(.defaultAction)
            }
        }
        .padding(20)
        .frame(width: 520)
    }
}

/// One model's name, size, accuracy and speed. Used by the picker and Settings.
struct ModelSummary: View {
    let model: SpeechModel
    let bytes: Int64
    var recommended = false

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack {
                Text(model.displayName).font(.headline)
                if recommended {
                    Text("Recommended")
                        .font(.caption.bold())
                        .padding(.horizontal, 6)
                        .padding(.vertical, 2)
                        .background(Capsule().fill(Color.accentColor.opacity(0.2)))
                }
            }
            Text("\(ModelLibrary.size(bytes)) download · \(model.languageNote) · \(model.source)")
                .font(.caption)
                .foregroundStyle(.secondary)
            Text(model.accuracyNote)
            Text(model.speedNote).foregroundStyle(.secondary)
        }
    }
}
