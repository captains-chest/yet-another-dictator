import SwiftUI
import TranscriptCleanup

/// Settings page for the optional transcript cleanup with S1-mini by Superwhisper (ADR 0011).
struct CleanupSection: View {
    let cleanup: CleanupSettings

    var body: some View {
        Section("Cleanup") {
            Toggle("Clean up with \(CleanupSettings.displayName)", isOn: Binding(
                get: { cleanup.isEnabled },
                set: { enabled in Task { await cleanup.setEnabled(enabled) } }
            ))
            Text("Removes filler words like \u{201C}um\u{201D}, applies spoken corrections (\u{201C}Friday, no wait, Thursday\u{201D}), and formats numbers, dates, times and email addresses. English only. Off by default; turning it on downloads the model (\(ModelLibrary.size(cleanup.bytes)), from Hugging Face, verified by SHA-256). It runs on this Mac.")
                .foregroundStyle(.secondary)
            if let status = cleanup.statusText {
                Text(status).foregroundStyle(statusIsFailure ? .red : .secondary)
            }
            if case .failed = cleanup.status {
                Button("Retry") { Task { await cleanup.retry() } }
            }
            Picker("Style", selection: Binding(get: { cleanup.styling }, set: { cleanup.styling = $0 })) {
                ForEach(CleanupStyle.Styling.allCases, id: \.self) { styling in
                    Text(styling.displayName).tag(styling)
                }
            }
            .disabled(!cleanup.isEnabled)
            Text("If cleanup is still loading, fails, or takes too long, the raw transcript is typed instead. Undo removes exactly what was typed.")
                .foregroundStyle(.secondary)
            if cleanup.canDelete {
                Button("Delete Cleanup Model (\(ModelLibrary.size(cleanup.bytes)))") { try? cleanup.delete() }
            }
            Text("\(CleanupSettings.displayName), licensed under the Apache License 2.0. Based on Qwen3-0.6B. Runs with llama.cpp (MIT).")
                .font(.caption)
                .foregroundStyle(.secondary)
        }
    }

    private var statusIsFailure: Bool {
        if case .failed = cleanup.status { return true }
        return false
    }
}

extension CleanupStyle.Styling {
    var displayName: String {
        switch self {
        case .casual: "Casual"
        case .semiCasual: "Semi-casual"
        case .semiFormal: "Semi-formal"
        case .formal: "Formal"
        }
    }
}
