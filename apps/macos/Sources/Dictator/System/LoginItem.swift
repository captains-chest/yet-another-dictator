import ServiceManagement

/// Opens Dictator at login via the system's login items (SMAppService), so dictation is always
/// available without opening the app by hand. macOS lists it under General → Login Items.
@MainActor
enum LoginItem {
    enum Status: Equatable {
        case enabled, disabled
        /// Registered, but the user must allow it in System Settings → General → Login Items.
        case needsApproval
    }

    static var status: Status {
        switch SMAppService.mainApp.status {
        case .enabled: .enabled
        case .requiresApproval: .needsApproval
        default: .disabled
        }
    }

    /// Returns a user-facing error message if macOS refused the change.
    static func set(enabled: Bool) -> String? {
        do {
            if enabled {
                try SMAppService.mainApp.register()
            } else {
                try SMAppService.mainApp.unregister()
            }
            return nil
        } catch {
            return "Couldn't change Open at Login: \(error.localizedDescription)"
        }
    }

    static func openSystemSettings() {
        SMAppService.openSystemSettingsLoginItems()
    }
}
