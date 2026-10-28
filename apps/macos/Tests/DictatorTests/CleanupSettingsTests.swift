import CryptoKit
import DictationCore
import Foundation
import Synchronization
import Testing
import TranscriptCleanup
import WhisperTranscription
@testable import Dictator

/// Answers every cleanup with a fixed text and counts how often it was prepared.
final class FakeTextCleaner: TranscriptCleaning {
    struct LoadFailed: Error {}

    let answer: String
    let failsToLoad: Bool
    private let prepared = Atomic<Int>(0)

    init(answer: String = "Cleaned.", failsToLoad: Bool = false) {
        self.answer = answer
        self.failsToLoad = failsToLoad
    }

    var prepareCount: Int { prepared.load(ordering: .relaxed) }

    func prepare() async throws {
        prepared.add(1, ordering: .relaxed)
        if failsToLoad { throw LoadFailed() }
    }

    func clean(_ transcript: String, style: CleanupStyle) async throws -> String { answer }
}

/// Always transcribes the same sentence.
struct CleanupFixedSpeech: Transcribing {
    let text: String
    func prepare() async throws {}
    func transcribe(_ samples: [Float], language: String?) async throws -> String { text }
}

/// The cleaners `makeCleaner` handed out, in order.
final class MadeCleaners: Sendable {
    private let made = Mutex<[URL]>([])
    func record(_ folder: URL) { made.withLock { $0.append(folder) } }
    var folders: [URL] { made.withLock { $0 } }
}

@MainActor
struct CleanupSettingsTests {
    static let files: [(path: String, local: String, data: Data)] = [
        ("test/s1/s1-mini-q4_k_m.gguf", "s1-mini-q4_k_m.gguf", Data("gguf weights".utf8)),
        ("test/s1/LICENSE", "LICENSE", Data("Apache 2.0".utf8)),
        ("test/s1/NOTICE", "NOTICE", Data("S1-mini by Superwhisper".utf8)),
    ]
    static let gguf = "test/s1/s1-mini-q4_k_m.gguf"

    static func testManifest() throws -> ModelManifest {
        let text = files.map { file in
            let hash = SHA256.hash(data: file.data).map { String(format: "%02x", $0) }.joined()
            return "s1-mini \(hash) \(file.data.count) \(file.path) \(file.local)"
        }.joined(separator: "\n")
        return try ModelManifest(text: text)
    }

    let root = FileManager.default.temporaryDirectory.appending(path: "CleanupSettingsTests-\(UUID().uuidString)", directoryHint: .isDirectory)
    let defaults = UserDefaults(suiteName: "CleanupSettingsTests-\(UUID().uuidString)")!
    let transport = FakeTransport(contents: Dictionary(uniqueKeysWithValues: files.map { ($0.path, $0.data) }))
    let made = MadeCleaners()
    let transcriber = CleaningTranscriber(base: CleanupFixedSpeech(text: "um send the report by friday no wait thursday"))

    func makeSettings(cleaner: FakeTextCleaner = FakeTextCleaner()) throws -> CleanupSettings {
        let made = made
        let settings = CleanupSettings(
            manifest: try Self.testManifest(), root: root, transport: transport, defaults: defaults,
            transcriber: transcriber,
            makeCleaner: { folder in
                made.record(folder)
                return cleaner
            }
        )
        settings.downloadAttempts = 1
        return settings
    }

    func waitForFetch(_ path: String) async {
        for _ in 0..<500 where transport.fetches(of: path) == 0 {
            try? await Task.sleep(for: .milliseconds(10))
        }
    }

    // MARK: - Opt-in

    @Test func isOffAndDownloadsNothingByDefault() async throws {
        let settings = try makeSettings()
        await settings.start()
        #expect(!settings.isEnabled)
        #expect(settings.status == .off)
        #expect(settings.styling == .semiFormal)
        #expect(transport.totalFetches == 0)
        #expect(made.folders.isEmpty)
        #expect(!transcriber.isCleaning)
    }

    @Test func turningItOnDownloadsLoadsAndInstallsTheCleaner() async throws {
        let cleaner = FakeTextCleaner()
        let settings = try makeSettings(cleaner: cleaner)
        await settings.setEnabled(true)
        #expect(settings.status == .ready)
        #expect(settings.isInstalled)
        #expect(transport.totalFetches == Self.files.count)
        #expect(cleaner.prepareCount == 1)
        #expect(made.folders == [root.appending(path: "s1-mini", directoryHint: .isDirectory)])
        #expect(transcriber.isCleaning)
        #expect(defaults.bool(forKey: CleanupSettings.enabledKey))
    }

    @Test func theChoiceIsRememberedAcrossLaunches() async throws {
        let first = try makeSettings()
        await first.setEnabled(true)
        first.styling = .casual

        let second = try makeSettings()
        #expect(second.isEnabled)
        #expect(second.styling == .casual)
        #expect(transcriber.style.styling == .casual)
        await second.start()
        #expect(second.status == .ready)
        // Already on disk: nothing fetched again.
        #expect(transport.totalFetches == Self.files.count)
    }

    @Test func turningItOffUnloadsTheCleanerAndKeepsTheDownload() async throws {
        let settings = try makeSettings()
        await settings.setEnabled(true)
        await settings.setEnabled(false)
        #expect(settings.status == .off)
        #expect(!transcriber.isCleaning)
        #expect(settings.isInstalled)
        #expect(settings.canDelete)
        try settings.delete()
        #expect(!settings.isInstalled)
    }

    @Test func theModelCannotBeDeletedWhileOn() async throws {
        let settings = try makeSettings()
        await settings.setEnabled(true)
        #expect(!settings.canDelete)
        try settings.delete()
        #expect(settings.isInstalled)
    }

    @Test func turningItOffDuringTheDownloadInstallsNothing() async throws {
        transport.hold(Self.gguf)
        let settings = try makeSettings()
        let enabling = Task { await settings.setEnabled(true) }
        await waitForFetch(Self.gguf)
        if case .downloading = settings.status {} else { Issue.record("expected downloading, got \(settings.status)") }
        await settings.setEnabled(false)
        transport.release(Self.gguf)
        await enabling.value
        #expect(settings.status == .off)
        #expect(made.folders.isEmpty)
        #expect(!transcriber.isCleaning)
    }

    @Test func aFailedDownloadTypesRawTextAndCanBeRetried() async throws {
        transport.setFailing(true)
        let settings = try makeSettings()
        await settings.setEnabled(true)
        guard case .failed = settings.status else {
            Issue.record("expected failed, got \(settings.status)")
            return
        }
        #expect(settings.statusText?.contains("raw text") == true)
        #expect(!transcriber.isCleaning)

        transport.setFailing(false)
        await settings.retry()
        #expect(settings.status == .ready)
        #expect(transcriber.isCleaning)
    }

    @Test func aModelThatFailsToLoadIsReportedAndNotUsed() async throws {
        let settings = try makeSettings(cleaner: FakeTextCleaner(failsToLoad: true))
        await settings.setEnabled(true)
        guard case .failed = settings.status else {
            Issue.record("expected failed, got \(settings.status)")
            return
        }
        #expect(!transcriber.isCleaning)
    }

    @Test func theStylingReachesTheTranscriber() throws {
        let settings = try makeSettings()
        settings.styling = .formal
        #expect(transcriber.style == CleanupStyle(styling: .formal))
        #expect(defaults.string(forKey: CleanupSettings.stylingKey) == "formal")
    }

    @Test func theModelIsShownUnderItsRequiredName() {
        #expect(CleanupSettings.displayName == "S1-mini by Superwhisper")
    }

    @Test func theBundledManifestPinsTheCleanupModel() throws {
        let manifest = try ModelLibraryTests.bundledManifest()
        #expect(manifest.totalBytes(for: CleanupSettings.modelID) == 484_219_808 + 12_033 + 507)
    }
}
