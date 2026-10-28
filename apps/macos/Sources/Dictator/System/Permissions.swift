import AppKit
import AVFoundation
import CoreGraphics

/// The three permissions Dictator needs, and nothing else.
@MainActor
enum Permissions {
    enum Status { case granted, denied, notDetermined }

    static var microphone: Status {
        switch AVCaptureDevice.authorizationStatus(for: .audio) {
        case .authorized: .granted
        case .notDetermined: .notDetermined
        default: .denied
        }
    }

    static func requestMicrophone() async -> Bool {
        await AVCaptureDevice.requestAccess(for: .audio)
    }

    /// Posting synthetic key events (listed under Accessibility in System Settings).
    /// The result is cached per process: a grant made while running only shows up after a relaunch.
    static var canPostEvents: Bool { CGPreflightPostEventAccess() }

    @discardableResult
    static func requestPostEvents() -> Bool { CGRequestPostEventAccess() }

    /// Seeing key presses from other apps, so any key combination can be the shortcut (Input Monitoring).
    static var canListenEvents: Bool { CGPreflightListenEventAccess() }

    @discardableResult
    static func requestListenEvents() -> Bool { CGRequestListenEventAccess() }

    static func relaunch() {
        let configuration = NSWorkspace.OpenConfiguration()
        configuration.createsNewApplicationInstance = true
        SingleInstance.markRelaunch()
        NSWorkspace.shared.openApplication(at: Bundle.main.bundleURL, configuration: configuration) { _, error in
            guard error == nil else { return }
            Task { @MainActor in NSApp.terminate(nil) }
        }
    }

    static func openPrivacySettings(_ pane: String) {
        guard let url = URL(string: "x-apple.systempreferences:com.apple.preference.security?\(pane)") else { return }
        NSWorkspace.shared.open(url)
    }
}
