import Foundation
import Testing
@testable import Dictator

@MainActor
final class FakeDeviceSource: AudioInputDeviceSource {
    var onChange: (() -> Void)?
    var devices: [AudioInputDevice] = []
    var systemDefault: AudioInputDevice?

    func start() {}
    func inputDevices() -> [AudioInputDevice] { devices }
    func systemDefaultInput() -> AudioInputDevice? { systemDefault }

    /// Simulates plugging in or removing a device.
    func change(devices: [AudioInputDevice]) {
        self.devices = devices
        onChange?()
    }
}

@MainActor
struct MicrophoneSettingsTests {
    let defaults = UserDefaults(suiteName: "MicrophoneSettingsTests-\(UUID().uuidString)")!
    let source = FakeDeviceSource()
    let headset = AudioInputDevice(id: 10, uid: "headset-uid", name: "Headset")
    let studio = AudioInputDevice(id: 11, uid: "studio-uid", name: "Studio Mic")

    func make() -> MicrophoneSettings {
        source.devices = [headset, studio]
        source.systemDefault = headset
        let settings = MicrophoneSettings(source: source, defaults: defaults)
        settings.start()
        return settings
    }

    @Test func followsSystemDefaultUntilADeviceIsChosen() {
        let settings = make()
        #expect(settings.choice == nil)
        #expect(settings.activeDevice == headset)
        #expect(!settings.isChoiceMissing)
    }

    @Test func chosenDeviceIsUsedAndRemembered() {
        let settings = make()
        var reported: [String?] = []
        settings.onChoiceChange = { reported.append($0) }

        settings.select(studio)

        #expect(settings.activeDevice == studio)
        #expect(reported == ["studio-uid"])
        #expect(make().choice == .init(uid: "studio-uid", name: "Studio Mic"))
    }

    @Test func choosingSystemDefaultForgetsTheDevice() {
        let settings = make()
        settings.select(studio)
        var reported: [String?] = []
        settings.onChoiceChange = { reported.append($0) }

        settings.select(nil)

        #expect(reported == [nil])
        #expect(defaults.data(forKey: MicrophoneSettings.choiceKey) == nil)
        #expect(settings.activeDevice == headset)
    }

    @Test func reselectingTheSameDeviceReportsNothing() {
        let settings = make()
        settings.select(studio)
        var reported: [String?] = []
        settings.onChoiceChange = { reported.append($0) }

        settings.select(studio)

        #expect(reported.isEmpty)
    }

    @Test func unpluggedChoiceFallsBackToDefaultAndReturnsWhenReconnected() {
        let settings = make()
        settings.select(studio)
        var changes = 0
        settings.onDevicesChange = { changes += 1 }

        source.change(devices: [headset])
        #expect(settings.isChoiceMissing)
        #expect(settings.activeDevice == headset)
        #expect(settings.choice?.uid == "studio-uid")

        // Core Audio gives a reconnected device a new ID; the UID stays.
        let reconnected = AudioInputDevice(id: 42, uid: "studio-uid", name: "Studio Mic")
        source.change(devices: [headset, reconnected])
        #expect(!settings.isChoiceMissing)
        #expect(settings.activeDevice == reconnected)
        #expect(changes == 2)
    }

    @Test func choiceRemembersADeviceThatIsNotConnectedAtLaunch() {
        make().select(studio)
        source.devices = [headset]
        let settings = MicrophoneSettings(source: source, defaults: defaults)
        settings.start()

        #expect(settings.choice?.name == "Studio Mic")
        #expect(settings.isChoiceMissing)
    }

    @Test func renamedDeviceUpdatesStoredName() {
        let settings = make()
        settings.select(studio)

        source.change(devices: [headset, AudioInputDevice(id: 11, uid: "studio-uid", name: "Desk Mic")])

        #expect(settings.choice?.name == "Desk Mic")
        #expect(MicrophoneSettings(source: source, defaults: defaults).choice?.name == "Desk Mic")
    }
}
