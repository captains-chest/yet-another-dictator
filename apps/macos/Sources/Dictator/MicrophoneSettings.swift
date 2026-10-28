import Foundation
import Observation

/// Where the input device list comes from; Core Audio in the app, a fake in tests.
@MainActor
protocol AudioInputDeviceSource: AnyObject {
    var onChange: (() -> Void)? { get set }
    func start()
    func inputDevices() -> [AudioInputDevice]
    func systemDefaultInput() -> AudioInputDevice?
}

extension AudioInputDeviceMonitor: AudioInputDeviceSource {
    func inputDevices() -> [AudioInputDevice] { AudioInputDevices.all() }
    func systemDefaultInput() -> AudioInputDevice? { AudioInputDevices.systemDefault() }
}

/// Which microphone dictation records from. Either the system default input, or one device chosen
/// in Settings, so dictation can use a dedicated microphone while calls use a headset. The choice
/// is kept while that device is unplugged; dictation uses the system default until it is back.
@MainActor
@Observable
final class MicrophoneSettings {
    /// A chosen device. The name is kept to show the choice while the device is unplugged.
    struct Choice: Codable, Equatable {
        let uid: String
        let name: String
    }

    private(set) var devices: [AudioInputDevice] = []
    private(set) var systemDefault: AudioInputDevice?
    /// nil follows the system default input.
    private(set) var choice: Choice?

    /// Called with the chosen device's UID (nil for the system default) when the choice changes.
    @ObservationIgnored var onChoiceChange: ((String?) -> Void)?
    /// Called when devices were plugged in or removed, or the system default changed.
    @ObservationIgnored var onDevicesChange: (() -> Void)?

    @ObservationIgnored private let source: any AudioInputDeviceSource
    @ObservationIgnored private let defaults: UserDefaults
    static let choiceKey = "microphone"

    init(source: any AudioInputDeviceSource, defaults: UserDefaults) {
        self.source = source
        self.defaults = defaults
        if let data = defaults.data(forKey: Self.choiceKey) {
            choice = try? JSONDecoder().decode(Choice.self, from: data)
        }
    }

    /// Starts watching for devices being plugged in or removed.
    func start() {
        source.onChange = { [weak self] in
            self?.refresh()
            self?.onDevicesChange?()
        }
        source.start()
        refresh()
    }

    /// True when a device was chosen but is not connected, so the system default is used.
    var isChoiceMissing: Bool {
        guard let choice else { return false }
        return Self.device(uid: choice.uid, in: devices) == nil
    }

    /// The device dictation records from right now.
    var activeDevice: AudioInputDevice? {
        Self.device(uid: choice?.uid, in: devices) ?? systemDefault
    }

    /// Pass nil to follow the system default input.
    func select(_ device: AudioInputDevice?) {
        let new = device.map { Choice(uid: $0.uid, name: $0.name) }
        guard new != choice else { return }
        choice = new
        save()
        onChoiceChange?(new?.uid)
    }

    /// The chosen device if it is connected; nil means record from the system default.
    nonisolated static func device(uid: String?, in devices: [AudioInputDevice]) -> AudioInputDevice? {
        guard let uid else { return nil }
        return devices.first { $0.uid == uid }
    }

    private func refresh() {
        devices = source.inputDevices()
        systemDefault = source.systemDefaultInput()
        // Keep the stored name current if the device was renamed (e.g. in Audio MIDI Setup).
        if let choice, let device = Self.device(uid: choice.uid, in: devices), device.name != choice.name {
            self.choice = Choice(uid: choice.uid, name: device.name)
            save()
        }
    }

    private func save() {
        if let choice, let data = try? JSONEncoder().encode(choice) {
            defaults.set(data, forKey: Self.choiceKey)
        } else {
            defaults.removeObject(forKey: Self.choiceKey)
        }
    }
}
