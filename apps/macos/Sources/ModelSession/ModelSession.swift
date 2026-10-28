import DictationCore
import Foundation
import Synchronization

/// A speech model once loaded; the only thing an adapter's engine has to implement.
public protocol LoadedSpeechModel: Sendable {
    func transcribe(_ samples: [Float], language: String?, stop: StopSignal) async throws -> String
}

/// Set when the caller gives up on a transcription; engines check it while decoding.
public final class StopSignal: Sendable {
    private let stopped = Mutex(false)
    public init() {}
    public var isSet: Bool { stopped.withLock { $0 } }
    func set() { stopped.withLock { $0 = true } }
}

/// Runs one speech model (ADR 0008): loads it once, shares that load between concurrent
/// `prepare` calls, and serialises transcriptions so a new one waits for an abandoned one.
public actor ModelSession: Transcribing {
    public enum Failure: Error, CustomStringConvertible {
        case missingModelFiles([String])
        public var description: String {
            switch self {
            case .missingModelFiles(let files): "Model is incomplete, missing: \(files.joined(separator: ", "))"
            }
        }
    }

    private let modelFolder: URL
    private let requiredFiles: [String]
    private let loadModel: @Sendable (URL) async throws -> any LoadedSpeechModel
    private var loaded: (any LoadedSpeechModel)?
    private var loading: Task<any LoadedSpeechModel, any Error>?
    /// The latest transcription; a new one waits for it.
    private var latest: Task<Void, Never>?

    public init(
        modelFolder: URL, requiredFiles: [String],
        load: @escaping @Sendable (URL) async throws -> any LoadedSpeechModel
    ) {
        self.modelFolder = modelFolder
        self.requiredFiles = requiredFiles
        self.loadModel = load
    }

    public func prepare() async throws {
        _ = try await ensureLoaded()
    }

    private func ensureLoaded() async throws -> any LoadedSpeechModel {
        if let loaded { return loaded }
        if let loading {
            return try await loading.value
        }
        let missing = requiredFiles.filter {
            !FileManager.default.fileExists(atPath: modelFolder.appending(path: $0).path(percentEncoded: false))
        }
        guard missing.isEmpty else { throw Failure.missingModelFiles(missing) }

        let folder = modelFolder
        let loadModel = loadModel
        let task = Task.detached(priority: .userInitiated) { try await loadModel(folder) }
        loading = task
        do {
            let model = try await task.value
            loaded = model
            return model
        } catch {
            loading = nil
            throw error
        }
    }

    public func transcribe(_ samples: [Float], language: String?) async throws -> String {
        let loaded = try await ensureLoaded()
        let previous = latest
        let stop = StopSignal()
        let work = Task {
            await previous?.value
            try Task.checkCancellation()
            return try await loaded.transcribe(samples, language: language, stop: stop)
        }
        latest = Task { _ = await work.result }
        return try await withTaskCancellationHandler {
            try await work.value
        } onCancel: {
            stop.set()
            work.cancel()
        }
    }
}
