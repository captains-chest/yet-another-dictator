import Foundation
import Synchronization
import TranscriptCleanup

/// S1-mini by Superwhisper behind the `TranscriptCleaning` port (ADR 0011).
///
/// Loads the GGUF from the verified model folder once, in the background, and cleans one
/// transcript at a time. A cancelled cleanup stops generating at the next token.
public actor S1MiniCleaner: TranscriptCleaning {
    public enum Failure: Error, CustomStringConvertible {
        case missingModelFile(String)

        public var description: String {
            switch self {
            case .missingModelFile(let name): "The cleanup model file \(name) is missing. Turn cleanup off and on to download it again."
            }
        }
    }

    /// The GGUF inside the model folder, as named in the manifest.
    public static let modelFileName = "s1-mini-q4_k_m.gguf"

    private let modelFile: URL
    private var loading: Task<LlamaModel, any Error>?
    /// The previous cleanup; the next one waits for it, so the model only ever runs one.
    private var latest: Task<Void, Never>?

    public init(modelFolder: URL) {
        modelFile = modelFolder.appending(path: Self.modelFileName)
    }

    public func prepare() async throws {
        _ = try await model()
    }

    public func clean(_ transcript: String, style: CleanupStyle) async throws -> String {
        let model = try await model()
        let previous = latest
        let stop = StopFlag()
        let work = Task.detached(priority: .userInitiated) { () throws -> String in
            await previous?.value
            try Task.checkCancellation()
            return try model.clean(transcript, style: style, stop: stop)
        }
        latest = Task { _ = await work.result }
        return try await withTaskCancellationHandler {
            try await work.value
        } onCancel: {
            stop.set()
            work.cancel()
        }
    }

    private func model() async throws -> LlamaModel {
        if let loading { return try await loading.value }
        let file = modelFile
        guard FileManager.default.fileExists(atPath: file.path(percentEncoded: false)) else {
            throw Failure.missingModelFile(file.lastPathComponent)
        }
        let task = Task.detached(priority: .userInitiated) { () throws -> LlamaModel in
            let model = try LlamaModel(file: file)
            // Warm up: the first pass sets up the GPU work, so the first dictation doesn't wait.
            _ = try? model.clean("um", style: .default, stop: StopFlag())
            return model
        }
        loading = task
        do {
            return try await task.value
        } catch {
            loading = nil
            throw error
        }
    }
}

/// Set when a cleanup is cancelled, read by the decode loop between tokens.
final class StopFlag: Sendable {
    private let flag = Atomic(false)
    func set() { flag.store(true, ordering: .relaxed) }
    var isSet: Bool { flag.load(ordering: .relaxed) }
}
