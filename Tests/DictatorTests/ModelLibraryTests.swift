import CryptoKit
import DictationCore
import Foundation
import Testing
import WhisperTranscription
@testable import Dictator

/// Writes known bytes per file, counts fetches per file, and can hold a model's first fetch.
final class FakeTransport: ModelTransport, @unchecked Sendable {
    struct Offline: Error {}

    private let lock = NSLock()
    private let contents: [String: Data]
    private var counts: [String: Int] = [:]
    private var held: Set<String> = []
    private var waiting: [String: CheckedContinuation<Void, Never>] = [:]
    private var failing = false

    init(contents: [String: Data]) { self.contents = contents }

    func fetches(of path: String) -> Int { lock.withLock { counts[path] ?? 0 } }
    var totalFetches: Int { lock.withLock { counts.values.reduce(0, +) } }
    func fetched(prefix: String) -> Int { lock.withLock { counts.filter { $0.key.hasPrefix(prefix) }.values.reduce(0, +) } }

    func hold(_ path: String) { lock.withLock { _ = held.insert(path) } }
    func setFailing(_ value: Bool) { lock.withLock { failing = value } }

    func release(_ path: String) {
        let continuation = lock.withLock { () -> CheckedContinuation<Void, Never>? in
            held.remove(path)
            return waiting.removeValue(forKey: path)
        }
        continuation?.resume()
    }

    func fetch(_ entry: ModelManifest.Entry, into part: URL, progress: @escaping @Sendable (Int64) -> Void) async throws {
        let (shouldHold, fail) = lock.withLock { () -> (Bool, Bool) in
            counts[entry.remotePath, default: 0] += 1
            return (held.contains(entry.remotePath), failing)
        }
        if shouldHold {
            await withCheckedContinuation { continuation in
                let resumeNow = lock.withLock { () -> Bool in
                    if !held.contains(entry.remotePath) { return true }
                    waiting[entry.remotePath] = continuation
                    return false
                }
                if resumeNow { continuation.resume() }
            }
        }
        if fail { throw Offline() }
        let data = contents[entry.remotePath]!
        try data.write(to: part)
        progress(Int64(data.count))
    }
}

/// Answers every transcription with its model's id.
struct FakeTranscriber: Transcribing {
    let model: SpeechModel
    func prepare() async throws {}
    func transcribe(_ samples: [Float], language: String?) async throws -> String { model.rawValue }
}

struct LoadBoom: Error, CustomStringConvertible { var description: String { "kaboom" } }

/// A transcriber whose `prepare` can be held open or made to fail.
final class GatedTranscriber: Transcribing, @unchecked Sendable {
    let model: SpeechModel
    private let lock = NSLock()
    private var error: (any Error)?
    private var held = false
    private var waiter: CheckedContinuation<Void, Never>?
    private var prepares = 0

    init(model: SpeechModel) { self.model = model }

    var prepareCount: Int { lock.withLock { prepares } }
    func hold() { lock.withLock { held = true } }
    func setError(_ error: (any Error)?) { lock.withLock { self.error = error } }

    func release() {
        let waiter = lock.withLock { () -> CheckedContinuation<Void, Never>? in
            held = false
            defer { self.waiter = nil }
            return self.waiter
        }
        waiter?.resume()
    }

    func prepare() async throws {
        let (error, held) = lock.withLock { () -> ((any Error)?, Bool) in
            prepares += 1
            return (self.error, held)
        }
        if let error { throw error }
        if held {
            await withCheckedContinuation { continuation in
                let resume = lock.withLock { () -> Bool in
                    if !held { return true }
                    waiter = continuation
                    return false
                }
                if resume { continuation.resume() }
            }
        }
    }

    func transcribe(_ samples: [Float], language: String?) async throws -> String { model.rawValue }
}

@MainActor
final class IntegrationRecorder: AudioRecording {
    var samples: [Float] = Array(repeating: 0.1, count: 16_000)
    private(set) var recording = false
    func start(onLevel: @escaping @Sendable (Float) -> Void) throws { recording = true }
    func stop() -> [Float] { recording = false; return samples }
}

@MainActor
final class IntegrationInserter: TextInserting {
    func preflight() throws {}
    func insert(_ text: String) async throws {}
    func deleteBackward(_ count: Int) async throws {}
}

@MainActor
final class IntegrationPreceding: PrecedingTextReading {
    func textBeforeCursor(maxLength: Int) -> String? { "" }
}

@MainActor
final class Activations {
    var count = 0
}

@MainActor
struct ModelLibraryTests {
    static let parakeet = SpeechModel.parakeetV2.rawValue
    static let whisper = SpeechModel.whisperLargeV3Turbo.rawValue

    /// remotePath -> bytes. Two files per model.
    static let files: [(model: String, path: String, data: Data)] = [
        (parakeet, "test/parakeet/encoder.bin", Data("parakeet encoder".utf8)),
        (parakeet, "test/parakeet/vocab.json", Data("parakeet vocab".utf8)),
        (whisper, "test/whisper/encoder.bin", Data("whisper encoder bytes".utf8)),
        (whisper, "test/whisper/tokenizer.json", Data("whisper tokenizer".utf8)),
    ]
    static let parakeetFirst = "test/parakeet/encoder.bin"
    static let whisperFirst = "test/whisper/encoder.bin"

    static func testManifest() throws -> ModelManifest {
        let text = files.map { file in
            let hash = SHA256.hash(data: file.data).map { String(format: "%02x", $0) }.joined()
            let local = file.path.split(separator: "/").last!
            return "\(file.model) \(hash) \(file.data.count) \(file.path) \(local)"
        }.joined(separator: "\n")
        return try ModelManifest(text: text)
    }

    let root = FileManager.default.temporaryDirectory.appending(path: "ModelLibraryTests-\(UUID().uuidString)", directoryHint: .isDirectory)
    let defaults = UserDefaults(suiteName: "ModelLibraryTests-\(UUID().uuidString)")!
    let transport = FakeTransport(contents: Dictionary(uniqueKeysWithValues: files.map { ($0.path, $0.data) }))
    let engine = ActiveTranscriber()
    let activations = Activations()

    func makeLibrary(
        transport: FakeTransport? = nil, gated: [SpeechModel: GatedTranscriber] = [:]
    ) throws -> ModelLibrary {
        let activations = activations
        let library = ModelLibrary(
            manifest: try Self.testManifest(), root: root, transport: transport ?? self.transport,
            defaults: defaults, engine: engine,
            makeTranscriber: { model, _ in
                if let gated = gated[model] { return gated }
                return FakeTranscriber(model: model)
            },
            onActivated: { activations.count += 1 }
        )
        library.downloadAttempts = 1
        return library
    }

    func folderExists(_ model: SpeechModel) -> Bool {
        FileManager.default.fileExists(atPath: root.appending(path: model.rawValue).path(percentEncoded: false))
    }

    func engineAnswer() async -> String? { try? await engine.transcribe([], language: nil) }

    func waitForFetch(_ path: String) async {
        for _ in 0..<500 where transport.fetches(of: path) == 0 {
            try? await Task.sleep(for: .milliseconds(10))
        }
    }

    // MARK: - Ported from SpeechModelTests

    static func bundledManifest() throws -> ModelManifest {
        let url = URL(filePath: #filePath)
            .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
            .appending(path: "scripts/model-manifest.txt")
        return try ModelManifest(text: String(contentsOf: url, encoding: .utf8))
    }

    @Test func rankingPutsParakeetFirstThenWhisper() {
        #expect(SpeechModel.ranked == [.parakeetV2, .whisperLargeV3Turbo])
    }

    @Test func everyModelIsPinnedInTheManifest() throws {
        let manifest = try Self.bundledManifest()
        for model in SpeechModel.allCases {
            #expect(!manifest.files(for: model.rawValue).isEmpty, "\(model.rawValue) missing from manifest")
        }
        #expect(manifest.files(for: SpeechModel.parakeetV2.rawValue).map(\.localPath).contains("parakeet_vocab.json"))
    }

    @Test func choicePersists() async throws {
        defer { try? FileManager.default.removeItem(at: root) }
        await (try makeLibrary()).choose(.parakeetV2)
        let reloaded = try makeLibrary()
        #expect(reloaded.selected == .parakeetV2)
        #expect(!reloaded.needsChoice)
        await reloaded.choose(.whisperLargeV3Turbo)
        #expect(try makeLibrary().selected == .whisperLargeV3Turbo)
        #expect(defaults.string(forKey: ModelLibrary.key) == Self.whisper)
    }

    @Test func unknownStoredModelCountsAsUnset() throws {
        defaults.set("some-removed-model", forKey: ModelLibrary.key)
        #expect(try makeLibrary().needsChoice)
    }

    @Test func sizesReadWell() {
        #expect(ModelLibrary.size(464_000_000) == "464 MB")
        #expect(ModelLibrary.size(1_625_000_000) == "1.6 GB")
    }

    // MARK: - Lifecycle

    @Test func freshLibraryNeedsChoiceAndFetchesNothing() async throws {
        defer { try? FileManager.default.removeItem(at: root) }
        let library = try makeLibrary()
        #expect(library.needsChoice)
        #expect(library.statusText == "No speech model chosen")
        await library.start()
        #expect(transport.totalFetches == 0)
        #expect(activations.count == 1)
        #expect(library.inUse == nil)
        #expect(!FileManager.default.fileExists(atPath: root.path(percentEncoded: false)))
    }

    @Test func installedWhisperIsAdoptedWithoutFetching() async throws {
        defer { try? FileManager.default.removeItem(at: root) }
        let installer = ModelInstaller(manifest: try Self.testManifest(), modelID: Self.whisper, root: root)
        try await installer.install(using: FakeTransport(contents: Dictionary(uniqueKeysWithValues: Self.files.map { ($0.path, $0.data) })))

        let library = try makeLibrary()
        #expect(library.selected == .whisperLargeV3Turbo)
        #expect(!library.needsChoice)
        await library.start()
        #expect(transport.totalFetches == 0)
        #expect(library.inUse == .whisperLargeV3Turbo)
        #expect(await engineAnswer() == Self.whisper)
    }

    @Test func choosingParakeetFetchesOnlyParakeet() async throws {
        defer { try? FileManager.default.removeItem(at: root) }
        let library = try makeLibrary()
        await library.choose(.parakeetV2)
        #expect(transport.fetched(prefix: "test/parakeet/") == 2)
        #expect(transport.fetched(prefix: "test/whisper/") == 0)
        #expect(library.inUse == .parakeetV2)
        #expect(library.status == .ready)
        #expect(activations.count == 1)
        #expect(await engineAnswer() == Self.parakeet)
        #expect(!folderExists(.whisperLargeV3Turbo))
    }

    @Test func switchingAwayAndBackJoinsTheRunningDownload() async throws {
        defer { try? FileManager.default.removeItem(at: root) }
        let library = try makeLibrary()
        transport.hold(Self.parakeetFirst)
        let first = Task { await library.choose(.parakeetV2) }
        await waitForFetch(Self.parakeetFirst)

        await library.choose(.whisperLargeV3Turbo)
        #expect(library.inUse == .whisperLargeV3Turbo)

        let again = Task { await library.choose(.parakeetV2) }
        await Task.yield()
        transport.release(Self.parakeetFirst)
        await first.value
        await again.value

        for file in Self.files where file.model == Self.parakeet {
            #expect(transport.fetches(of: file.path) == 1, "\(file.path) fetched more than once")
        }
        #expect(library.isInstalled(.parakeetV2))
        #expect(library.inUse == .parakeetV2)
        #expect(library.selected == .parakeetV2)
        #expect(await engineAnswer() == Self.parakeet)
    }

    @Test func modelInUseCannotBeDeletedUntilAnotherIsActive() async throws {
        defer { try? FileManager.default.removeItem(at: root) }
        let library = try makeLibrary()
        await library.choose(.whisperLargeV3Turbo)
        transport.hold(Self.parakeetFirst)
        let switching = Task { await library.choose(.parakeetV2) }
        await waitForFetch(Self.parakeetFirst)

        // Whisper is no longer selected but still transcribing; Parakeet is still downloading.
        #expect(library.inUse == .whisperLargeV3Turbo)
        #expect(!library.canDelete(.whisperLargeV3Turbo))
        #expect(!library.canDelete(.parakeetV2))
        try library.delete(.whisperLargeV3Turbo)
        #expect(folderExists(.whisperLargeV3Turbo))
        #expect(library.isInstalled(.whisperLargeV3Turbo))

        transport.release(Self.parakeetFirst)
        await switching.value
        #expect(library.inUse == .parakeetV2)
        #expect(library.canDelete(.whisperLargeV3Turbo))
        #expect(!library.canDelete(.parakeetV2))
        try library.delete(.whisperLargeV3Turbo)
        #expect(!folderExists(.whisperLargeV3Turbo))
    }

    @Test func failedDownloadKeepsThePreviousEngineAndRetryResumes() async throws {
        defer { try? FileManager.default.removeItem(at: root) }
        let library = try makeLibrary()
        await library.choose(.whisperLargeV3Turbo)
        transport.setFailing(true)
        await library.choose(.parakeetV2)
        guard case .failed = library.status else {
            Issue.record("expected failed, got \(library.status)")
            return
        }
        #expect(library.statusText?.hasPrefix("Model download failed") == true)
        #expect(library.inUse == .whisperLargeV3Turbo)
        #expect(await engineAnswer() == Self.whisper)

        transport.setFailing(false)
        await library.retry()
        #expect(library.status == .ready)
        #expect(library.inUse == .parakeetV2)
        #expect(await engineAnswer() == Self.parakeet)
    }

    @Test func progressOfAnUnselectedModelNeverChangesStatus() async throws {
        defer { try? FileManager.default.removeItem(at: root) }
        let library = try makeLibrary()
        transport.hold(Self.parakeetFirst)
        let abandoned = Task { await library.choose(.parakeetV2) }
        await waitForFetch(Self.parakeetFirst)

        await library.choose(.whisperLargeV3Turbo)
        #expect(library.status == .ready)
        transport.release(Self.parakeetFirst)
        await abandoned.value
        for _ in 0..<20 { await Task.yield() }
        #expect(library.status == .ready)
        #expect(library.statusText == nil)
        #expect(library.inUse == .whisperLargeV3Turbo)
        #expect(library.selected == .whisperLargeV3Turbo)
    }
}

@MainActor
extension ModelLibraryTests {
    func waitForPrepare(_ transcriber: GatedTranscriber) async {
        for _ in 0..<500 where transcriber.prepareCount == 0 {
            try? await Task.sleep(for: .milliseconds(10))
        }
    }

    @Test func switchKeepsOldServingWhileNewLoads() async throws {
        defer { try? FileManager.default.removeItem(at: root) }
        let parakeet = GatedTranscriber(model: .parakeetV2)
        let library = try makeLibrary(gated: [.parakeetV2: parakeet])
        await library.choose(.whisperLargeV3Turbo)
        #expect(activations.count == 1)

        parakeet.hold()
        let switching = Task { await library.choose(.parakeetV2) }
        await waitForPrepare(parakeet)
        #expect(library.inUse == .whisperLargeV3Turbo)
        #expect(await engineAnswer() == Self.whisper)

        parakeet.release()
        await switching.value
        #expect(library.inUse == .parakeetV2)
        #expect(await engineAnswer() == Self.parakeet)
        // A switch never re-activates the controller.
        #expect(activations.count == 1)
    }

    @Test func failedLoadKeepsPreviousAndOffersRetry() async throws {
        defer { try? FileManager.default.removeItem(at: root) }
        let parakeet = GatedTranscriber(model: .parakeetV2)
        parakeet.setError(LoadBoom())
        let library = try makeLibrary(gated: [.parakeetV2: parakeet])
        await library.choose(.whisperLargeV3Turbo)
        await library.choose(.parakeetV2)

        #expect(library.loadFailure == "kaboom")
        #expect(library.statusText?.hasPrefix("Couldn't load NVIDIA Parakeet TDT 0.6B v2: kaboom") == true)
        #expect(library.statusText?.hasSuffix("Still using Whisper Large v3 Turbo.") == true)
        #expect(library.canRetry)
        #expect(library.inUse == .whisperLargeV3Turbo)
        #expect(await engineAnswer() == Self.whisper)
        #expect(activations.count == 1)

        parakeet.setError(nil)
        await library.retry()
        #expect(library.inUse == .parakeetV2)
        #expect(library.loadFailure == nil)
        #expect(!library.canRetry)
        #expect(library.statusText == nil)
    }

    @Test func failedFirstLoadTellsControllerWhy() async throws {
        defer { try? FileManager.default.removeItem(at: root) }
        let parakeet = GatedTranscriber(model: .parakeetV2)
        parakeet.setError(LoadBoom())
        let library = try makeLibrary(gated: [.parakeetV2: parakeet])
        await library.choose(.parakeetV2)

        #expect(activations.count == 1)
        #expect(library.inUse == nil)
        do {
            try await engine.prepare()
            Issue.record("expected the load failure")
        } catch {
            #expect("\(error)".contains("kaboom"), "got \(error)")
            #expect(!"\(error)".contains("Choose a speech model"), "got \(error)")
        }

        parakeet.setError(nil)
        await library.retry()
        #expect(library.inUse == .parakeetV2)
        #expect(activations.count == 2)
    }

    @Test func returningToServingModelDropsLoadingOne() async throws {
        defer { try? FileManager.default.removeItem(at: root) }
        let parakeet = GatedTranscriber(model: .parakeetV2)
        let library = try makeLibrary(gated: [.parakeetV2: parakeet])
        await library.choose(.whisperLargeV3Turbo)

        parakeet.hold()
        let switching = Task { await library.choose(.parakeetV2) }
        await waitForPrepare(parakeet)
        await library.choose(.whisperLargeV3Turbo)
        parakeet.release()
        await switching.value

        #expect(library.selected == .whisperLargeV3Turbo)
        #expect(library.inUse == .whisperLargeV3Turbo)
        #expect(await engineAnswer() == Self.whisper)
        #expect(activations.count == 1)
    }

    @Test func switchingDuringRecordingDoesNotWipeIt() async throws {
        defer { try? FileManager.default.removeItem(at: root) }
        let recorder = IntegrationRecorder()
        let activations = Activations()
        let controller = DictationController(
            recorder: recorder, transcriber: engine, inserter: IntegrationInserter(),
            precedingText: IntegrationPreceding(), stopTail: .zero
        )
        let library = ModelLibrary(
            manifest: try Self.testManifest(), root: root, transport: transport,
            defaults: defaults, engine: engine,
            makeTranscriber: { model, _ in FakeTranscriber(model: model) },
            onActivated: {
                activations.count += 1
                await controller.loadModel()
            }
        )
        library.downloadAttempts = 1
        await library.choose(.whisperLargeV3Turbo)
        #expect(controller.state == .ready)

        controller.hotkeyPressed()
        #expect(controller.state == .recording)
        #expect(recorder.recording)

        await library.choose(.parakeetV2)
        #expect(library.inUse == .parakeetV2)
        #expect(controller.state == .recording)
        #expect(recorder.recording)
        #expect(activations.count == 1)
    }
}
