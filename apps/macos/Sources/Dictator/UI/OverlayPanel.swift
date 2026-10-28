import AppKit
import DictationCore
import SwiftUI

/// Small floating pill shown while listening/transcribing. Never takes focus or mouse input, so
/// the text field you're dictating into stays focused.
@MainActor
final class OverlayPanel {
    private let panel: NSPanel
    private let notice = Notice()
    private var hideNotice: Task<Void, Never>?
    private var isDictating = false

    /// Messages shown while not dictating. Listening/transcribing always covers both.
    @Observable final class Notice {
        /// Timed, e.g. why dictation couldn't start or type.
        var flash: String?
        /// Informational flashes (e.g. an update) instead of a warning.
        var flashIsInfo = false
        /// Sticky update progress or result. Shown whenever no flash covers it.
        var status: String?
    }

    init(controller: DictationController) {
        panel = NSPanel(
            contentRect: NSRect(x: 0, y: 0, width: OverlayView.width, height: OverlayView.height),
            styleMask: [.nonactivatingPanel, .borderless],
            backing: .buffered,
            defer: true
        )
        panel.level = .statusBar
        panel.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary, .stationary, .ignoresCycle]
        panel.isOpaque = false
        panel.backgroundColor = .clear
        panel.hasShadow = true
        panel.ignoresMouseEvents = true
        panel.hidesOnDeactivate = false
        panel.contentView = NSHostingView(rootView: OverlayView(controller: controller, notice: notice))
    }

    func update(for state: DictationState) {
        switch state {
        case .recording, .transcribing:
            hideNotice?.cancel()
            notice.flash = nil
            isDictating = true
        case .ready, .loadingModel, .unavailable:
            isDictating = false
        }
        refresh()
    }

    /// Stays up until replaced or cleared with nil. Used while an update downloads and installs.
    func present(_ status: String?) {
        notice.status = status
        refresh()
    }

    /// Shows `message` for a few seconds, e.g. why dictation couldn't start or type.
    func flash(_ message: String, isInfo: Bool = false) {
        notice.flash = message
        notice.flashIsInfo = isInfo
        refresh()
        hideNotice?.cancel()
        hideNotice = Task { [weak self] in
            try? await Task.sleep(for: .seconds(4))
            guard let self, !Task.isCancelled else { return }
            notice.flash = nil
            refresh()
        }
    }

    private func refresh() {
        if isDictating || notice.flash != nil || notice.status != nil {
            show()
        } else {
            panel.orderOut(nil)
        }
    }

    private func show() {
        if !panel.isVisible { position() }
        panel.orderFrontRegardless()
    }

    /// Bottom-centre of the screen the mouse is on.
    private func position() {
        let mouse = NSEvent.mouseLocation
        guard let screen = NSScreen.screens.first(where: { $0.frame.contains(mouse) }) ?? NSScreen.main else { return }
        let visible = screen.visibleFrame
        let size = panel.frame.size
        panel.setFrameOrigin(NSPoint(x: visible.midX - size.width / 2, y: visible.minY + 48))
    }
}

private struct OverlayView: View {
    static let width: CGFloat = 360
    static let height: CGFloat = 84

    let controller: DictationController
    let notice: OverlayPanel.Notice

    private var isDictating: Bool {
        controller.state == .recording || controller.state == .transcribing
    }

    var body: some View {
        // The panel is transparent outside the pill; the pill sizes to its content.
        pill
            .frame(width: Self.width, height: Self.height, alignment: .bottom)
    }

    private var pill: some View {
        HStack(spacing: 10) {
            if !isDictating, let text = notice.flash ?? notice.status {
                if notice.flash == nil || notice.flashIsInfo {
                    Image(systemName: "arrow.down.circle.fill").foregroundStyle(.blue)
                } else {
                    Image(systemName: "exclamationmark.triangle.fill").foregroundStyle(.yellow)
                }
                Text(text)
                    .lineLimit(3)
                    .fixedSize(horizontal: false, vertical: true)
            } else if controller.state == .transcribing {
                ProgressView().controlSize(.small)
                Text("Transcribing")
            } else {
                Circle().fill(.red).frame(width: 8, height: 8)
                Text("Listening")
                LevelBars(level: controller.level)
            }
        }
        .font(.system(size: 13, weight: .medium))
        .foregroundStyle(.white)
        .padding(.horizontal, 16)
        .padding(.vertical, 8)
        .frame(minWidth: 200, maxWidth: Self.width, minHeight: 44)
        .background(.black.opacity(0.8), in: RoundedRectangle(cornerRadius: 22))
    }
}

private struct LevelBars: View {
    let level: Float
    private let weights: [Float] = [0.5, 0.8, 1.0, 0.8, 0.5]

    var body: some View {
        HStack(spacing: 3) {
            ForEach(weights.indices, id: \.self) { index in
                Capsule()
                    .fill(.white)
                    .frame(width: 3, height: CGFloat(4 + 16 * min(1, level * weights[index])))
            }
        }
        .frame(height: 20)
        .animation(.easeOut(duration: 0.08), value: level)
    }
}
