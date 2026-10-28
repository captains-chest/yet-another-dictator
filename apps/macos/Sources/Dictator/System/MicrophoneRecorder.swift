@preconcurrency import AVFoundation
import DictationCore

/// Microphone capture with AVAudioEngine, resampled to 16 kHz mono Float32.
///
/// Audio lives only in memory and is never written to disk. The engine is driven from its own
/// serial queue, so starting it (which can take a few hundred milliseconds) never blocks the main
/// thread or the shortcut. It is built ahead of time, and rebuilt when the input device changes.
///
/// It records from the device chosen in Settings, or the system default input. While the chosen
/// device is unplugged it falls back to the system default, and switches back once it returns.
///
/// With "Keep microphone ready" on, the engine keeps running between dictations and the last
/// `preRollSeconds` are held in a ring buffer, so a dictation includes the first syllable even if
/// the user starts speaking as they press the shortcut. Off by default, because macOS then shows
/// the microphone indicator the whole time.
@MainActor
final class MicrophoneRecorder: AudioRecording {
    enum Failure: LocalizedError {
        case permissionDenied
        var errorDescription: String? {
            "Microphone access is denied. Enable it in System Settings → Privacy & Security → Microphone."
        }
    }

    /// Hard cap so a forgotten recording can't grow without bound (10 minutes ≈ 38 MB).
    private static let maxSamples = Int(DictationController.sampleRate) * 60 * 10
    static let preRollSeconds = 0.4

    /// Called on the main actor when the microphone could not start or stopped on its own.
    var onFailure: ((String) -> Void)?
    private(set) var keepsReady = false

    private let buffer = CaptureBuffer(
        capacity: maxSamples,
        preRoll: Int(DictationController.sampleRate * preRollSeconds)
    )
    /// Called on the main actor when a recording hit the 10-minute cap and stopped growing.
    var onCapacityReached: (() -> Void)?
    private lazy var host: AudioEngineHost = {
        // Fires on the realtime audio thread: only hop to the main actor, never block.
        buffer.onCapacityReached = { [weak self] in
            Task { @MainActor in self?.onCapacityReached?() }
        }
        return AudioEngineHost(buffer: buffer) { [weak self] message in
            Task { @MainActor in self?.onFailure?(message) }
        }
    }()

    /// Builds the engine without starting it, so the first dictation starts faster.
    /// Call once microphone access is granted.
    func prepare() {
        guard Permissions.microphone == .granted else { return }
        host.prepare()
    }

    /// Keeps the engine running between dictations, feeding the pre-roll buffer.
    func setKeepsReady(_ on: Bool) {
        keepsReady = on
        guard Permissions.microphone == .granted else { return }
        if on {
            host.run()
        } else if !buffer.isCapturing {
            host.idle()
        }
    }

    /// Records from the device with this UID when it is connected; nil follows the system default.
    func setPreferredDevice(uid: String?) {
        host.setPreferredDevice(uid: uid)
    }

    /// A device was plugged in or removed: switch to or away from the chosen device if needed.
    func inputDevicesChanged() {
        host.inputDevicesChanged()
    }

    func start(onLevel: @escaping @Sendable (Float) -> Void) throws {
        guard Permissions.microphone != .denied else { throw Failure.permissionDenied }
        buffer.begin(onLevel: onLevel)
        host.run()
    }

    func stop() -> [Float] {
        let samples = buffer.end()
        if !keepsReady { host.idle() }
        return samples
    }
}

/// Owns the AVAudioEngine. All of its state is touched only on `queue`.
private final class AudioEngineHost: @unchecked Sendable {
    private let queue = DispatchQueue(label: "com.captainduckay.dictator.audio", qos: .userInitiated)
    private let buffer: CaptureBuffer
    private let onFailure: @Sendable (String) -> Void
    private var engine: AVAudioEngine?
    private var configurationObserver: (any NSObjectProtocol)?
    /// Whether the engine should be running (recording, or kept ready).
    private var wantsRunning = false
    private var rebuildPending = false
    /// UID of the device chosen in Settings; nil follows the system default input.
    private var preferredUID: String?
    /// The device the engine records from when it is not the system default.
    private var builtDevice: AudioDeviceID?
    /// The input format the tap and converter were built for.
    private var builtFormat: AVAudioFormat?

    init(buffer: CaptureBuffer, onFailure: @escaping @Sendable (String) -> Void) {
        self.buffer = buffer
        self.onFailure = onFailure
    }

    func prepare() {
        queue.async { [self] in
            if engine == nil { try? build() }
        }
    }

    func run() {
        queue.async { [self] in
            wantsRunning = true
            startIfNeeded()
        }
    }

    func setPreferredDevice(uid: String?) {
        queue.async { [self] in
            guard uid != preferredUID else { return }
            preferredUID = uid
            if engine != nil || wantsRunning { rebuild() }
        }
    }

    func inputDevicesChanged() {
        queue.async { [self] in
            // Following the system default is handled by the configuration change notification.
            guard engine != nil, preferredUID != nil else { return }
            if chosenDevice()?.id != builtDevice { scheduleRebuild() }
        }
    }

    /// The chosen device if it is connected; nil means the system default.
    private func chosenDevice() -> AudioInputDevice? {
        MicrophoneSettings.device(uid: preferredUID, in: AudioInputDevices.all())
    }

    /// Stops the engine but keeps it built for the next start.
    func idle() {
        queue.async { [self] in
            wantsRunning = false
            engine?.stop()
            buffer.clearPreRoll()
        }
    }

    private func startIfNeeded() {
        do {
            if engine == nil { try build() }
            guard let engine, !engine.isRunning else { return }
            try engine.start()
        } catch {
            teardown()
            wantsRunning = false
            onFailure("Could not start microphone: \(error.localizedDescription)")
        }
    }

    private func build() throws {
        teardown()
        let engine = AVAudioEngine()
        let input = engine.inputNode
        // Choose the device before reading the format, so the format is the chosen device's.
        let device = chosenDevice()
        if let device {
            var id = device.id
            guard
                let unit = input.audioUnit,
                AudioUnitSetProperty(
                    unit,
                    kAudioOutputUnitProperty_CurrentDevice,
                    kAudioUnitScope_Global,
                    0,
                    &id,
                    UInt32(MemoryLayout<AudioDeviceID>.size)
                ) == noErr
            else { throw Failure.deviceUnavailable(device.name) }
        }
        let inputFormat = input.outputFormat(forBus: 0)
        guard inputFormat.sampleRate > 0, inputFormat.channelCount > 0 else { throw Failure.noInputDevice }
        guard
            let outputFormat = AVAudioFormat(
                commonFormat: .pcmFormatFloat32,
                sampleRate: DictationController.sampleRate,
                channels: 1,
                interleaved: false
            ),
            let converter = AVAudioConverter(from: inputFormat, to: outputFormat)
        else { throw Failure.converterUnavailable }
        // Mix all channels: on an audio interface the microphone may not be on the first input.
        converter.downmix = true

        input.installTap(
            onBus: 0,
            bufferSize: 1024,
            format: inputFormat,
            block: Self.makeTap(converter: converter, outputFormat: outputFormat, buffer: buffer)
        )
        // Fires when the input device or its format changes (headset plugged in, default input
        // switched). The engine has stopped by then; rebuild it for the new device.
        configurationObserver = NotificationCenter.default.addObserver(
            forName: .AVAudioEngineConfigurationChange,
            object: engine,
            queue: nil
        ) { [weak self] _ in
            guard let self else { return }
            queue.async { self.scheduleRebuild() }
        }
        engine.prepare()
        self.engine = engine
        builtDevice = device?.id
        builtFormat = inputFormat
    }

    /// Device switches often send several notifications in a row; rebuild once they settle.
    private func scheduleRebuild() {
        guard !rebuildPending else { return }
        rebuildPending = true
        queue.asyncAfter(deadline: .now() + 0.25) { [self] in
            rebuildPending = false
            if !isCurrent { rebuild() }
        }
    }

    /// With a chosen device, the engine reports a configuration change every time it starts,
    /// though nothing changed. Rebuilding then would restart it, and loop. Skip those.
    private var isCurrent: Bool {
        guard let engine, let builtDevice, let builtFormat else { return false }
        return chosenDevice()?.id == builtDevice
            && engine.inputNode.outputFormat(forBus: 0) == builtFormat
            && (engine.isRunning || !wantsRunning)
    }

    private func rebuild() {
        teardown()
        if wantsRunning {
            startIfNeeded()
        } else {
            try? build()
        }
    }

    private func teardown() {
        if let configurationObserver {
            NotificationCenter.default.removeObserver(configurationObserver)
        }
        configurationObserver = nil
        if let engine {
            engine.inputNode.removeTap(onBus: 0)
            engine.stop()
        }
        engine = nil
        builtDevice = nil
        builtFormat = nil
        buffer.clearPreRoll()
    }

    private enum Failure: LocalizedError {
        case noInputDevice, converterUnavailable
        case deviceUnavailable(String)
        var errorDescription: String? {
            switch self {
            case .noInputDevice: "No microphone input is available."
            case .deviceUnavailable(let name): "Cannot use “\(name)”."
            case .converterUnavailable: "Cannot convert microphone audio to 16 kHz."
            }
        }
    }

    /// Built outside any actor: the tap runs on a realtime audio thread.
    private static func makeTap(
        converter: AVAudioConverter,
        outputFormat: AVAudioFormat,
        buffer store: CaptureBuffer
    ) -> AVAudioNodeTapBlock {
        let ratio = outputFormat.sampleRate / converter.inputFormat.sampleRate
        return { buffer, _ in
            let capacity = AVAudioFrameCount((Double(buffer.frameLength) * ratio).rounded(.up)) + 1
            guard let output = AVAudioPCMBuffer(pcmFormat: outputFormat, frameCapacity: capacity) else { return }

            var consumed = false
            var error: NSError?
            converter.convert(to: output, error: &error) { _, status in
                if consumed {
                    status.pointee = .noDataNow
                    return nil
                }
                consumed = true
                status.pointee = .haveData
                return buffer
            }
            guard error == nil, let channel = output.floatChannelData?[0] else { return }
            store.append(UnsafeBufferPointer(start: channel, count: Int(output.frameLength)))
        }
    }
}
