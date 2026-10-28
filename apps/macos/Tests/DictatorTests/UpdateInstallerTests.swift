import AppUpdate
import CryptoKit
import DictationCore
import Foundation
import Testing
@testable import Dictator

/// Stands in for the network, `ditto`, code signing and the updater. Writes real files so
/// `ArchiveIntegrity` still checks a real checksum.
final class FakeInstallSteps: InstallSteps, @unchecked Sendable {
    struct Offline: Error {}

    static let zipBytes = Data("pretend this is Dictator.app zipped".utf8)

    private let lock = NSLock()
    private var _calls: [String] = []
    private var _replaced = false

    var failChecksumDownload = false
    var failZipDownload = false
    var wrongChecksum = false
    var stagedTrust = FakeInstallSteps.goodTrust(version: "0.2.0")
    var running = AppIdentity(bundleIdentifier: DictatorIdentity.bundleIdentifier, version: AppVersion("0.1.3")!)
    /// What the running bundle reads as once `replace` has been called.
    var runningAfterReplace: AppIdentity?
    var reply: UpdaterReply = .replied(nil)

    static func goodTrust(version: String) -> ReleaseTrust {
        ReleaseTrust(
            identity: AppIdentity(bundleIdentifier: DictatorIdentity.bundleIdentifier, version: AppVersion(version)!),
            teamIdentifier: DictatorIdentity.teamIdentifier,
            signatureValid: true,
            hardenedRuntime: true,
            stapledTicket: true
        )
    }

    var calls: [String] { lock.withLock { _calls } }

    private func record(_ call: String) { lock.withLock { _calls.append(call) } }

    func download(_ url: URL, to destination: URL, limit: Int, progress: @escaping @Sendable (Double) -> Void) async throws {
        if url.pathExtension == "sha256" {
            record("download checksum")
            if failChecksumDownload { throw Offline() }
            let hash = wrongChecksum
                ? String(repeating: "0", count: 64)
                : SHA256.hash(data: Self.zipBytes).map { String(format: "%02x", $0) }.joined()
            try Data("\(hash)  \(url.deletingPathExtension().lastPathComponent)\n".utf8).write(to: destination)
        } else {
            record("download zip")
            if failZipDownload { throw Offline() }
            progress(0.5)
            try Self.zipBytes.write(to: destination)
            progress(1)
        }
    }

    func extractApp(from zip: URL, into directory: URL) throws -> URL {
        record("extract")
        let app = directory.appending(path: "Dictator.app", directoryHint: .isDirectory)
        try FileManager.default.createDirectory(at: app, withIntermediateDirectories: true)
        return app
    }

    func trust(at app: URL) -> ReleaseTrust {
        record("trust")
        return stagedTrust
    }

    func runningIdentity() -> AppIdentity {
        record("runningIdentity")
        return lock.withLock { _replaced ? (runningAfterReplace ?? running) : running }
    }

    func replace(stagedApp: URL) async -> UpdaterReply {
        record("replace")
        lock.withLock { _replaced = true }
        return reply
    }
}

final class PhaseRecorder: @unchecked Sendable {
    private let lock = NSLock()
    private var _phases: [UpdatePhase] = []
    var phases: [UpdatePhase] { lock.withLock { _phases } }
    var report: @Sendable (UpdatePhase) -> Void { { phase in self.lock.withLock { self._phases.append(phase) } } }
}

struct UpdateInstallerTests {
    let workRoot = FileManager.default.temporaryDirectory.appending(path: "UpdateInstallerTests-\(UUID().uuidString)", directoryHint: .isDirectory)
    let fake = FakeInstallSteps()
    let phases = PhaseRecorder()

    static let base = "https://github.com/captains-chest/yet-another-dictator/releases/download/v0.2.0/"
    let update = OfferedUpdate(
        version: AppVersion("0.2.0")!,
        pageURL: URL(string: "https://github.com/captains-chest/yet-another-dictator/releases/tag/v0.2.0")!,
        zipURL: URL(string: base + "Dictator-0.2.0.zip")!,
        checksumURL: URL(string: base + "Dictator-0.2.0.zip.sha256")!,
        zipBytes: 35
    )

    init() throws {
        try FileManager.default.createDirectory(at: workRoot, withIntermediateDirectories: true)
    }

    func install(_ update: OfferedUpdate? = nil) async throws {
        try await UpdateInstaller(steps: fake, workRoot: workRoot).install(update ?? self.update, progress: phases.report)
    }

    func expectFailure(_ expected: UpdateInstallError, _ update: OfferedUpdate? = nil) async {
        await #expect(throws: expected) { try await install(update) }
    }

    func leftoverWork() throws -> [String] {
        try FileManager.default.contentsOfDirectory(atPath: workRoot.path)
    }

    @Test func happyPathRunsStepsInOrder() async throws {
        try await install()
        #expect(fake.calls == ["download checksum", "download zip", "extract", "trust", "runningIdentity", "replace"])
        #expect(phases.phases == [.downloading(0), .downloading(0.5), .downloading(1), .checking, .installing])
        #expect(try leftoverWork().isEmpty)
    }

    @Test func missingAssetsThrowArchiveWithoutDownloading() async throws {
        let bare = OfferedUpdate(version: update.version, pageURL: update.pageURL, zipURL: update.zipURL, checksumURL: nil, zipBytes: nil)
        await expectFailure(.archive, bare)
        #expect(fake.calls.isEmpty)
        #expect(try leftoverWork().isEmpty)
    }

    @Test func checksumDownloadFailureNeverFetchesZip() async throws {
        fake.failChecksumDownload = true
        await expectFailure(.download)
        #expect(fake.calls == ["download checksum"])
        #expect(try leftoverWork().isEmpty)
    }

    @Test func zipDownloadFailure() async throws {
        fake.failZipDownload = true
        await expectFailure(.download)
        #expect(fake.calls == ["download checksum", "download zip"])
        #expect(try leftoverWork().isEmpty)
    }

    @Test func checksumMismatchNeverExtractsOrReplaces() async throws {
        fake.wrongChecksum = true
        await expectFailure(.checksum)
        #expect(fake.calls == ["download checksum", "download zip"])
        #expect(!phases.phases.contains(.checking))
    }

    @Test func badSignatureNeverReachesUpdater() async {
        fake.stagedTrust.stapledTicket = false
        await expectFailure(.rejected(InstallPolicy.untrusted))
        #expect(!fake.calls.contains("replace"))
        #expect(!phases.phases.contains(.installing))
    }

    @Test func sameOrOlderVersionNeverReachesUpdater() async {
        fake.stagedTrust = FakeInstallSteps.goodTrust(version: "0.1.3")
        await expectFailure(.rejected("Dictator 0.1.3 is already current."))
        #expect(!fake.calls.contains("replace"))

        fake.stagedTrust = FakeInstallSteps.goodTrust(version: "0.1.0")
        await expectFailure(.rejected("Dictator 0.1.3 is already current."))
        #expect(!fake.calls.contains("replace"))
    }

    @Test func unreadableRunningVersionIsRefused() async {
        fake.running = AppIdentity(bundleIdentifier: DictatorIdentity.bundleIdentifier, version: nil)
        await expectFailure(.rejected("The update's version couldn't be read."))
        #expect(!fake.calls.contains("replace"))
    }

    @Test func updaterRefusalIsShown() async throws {
        fake.reply = .replied("Dictator can only replace its own copy.")
        await expectFailure(.rejected("Dictator can only replace its own copy."))
        #expect(fake.calls.last == "replace")
        #expect(try leftoverWork().isEmpty)
    }

    @Test func timeoutAfterReplaceCountsAsSuccess() async throws {
        fake.reply = .timedOut
        fake.runningAfterReplace = AppIdentity(bundleIdentifier: DictatorIdentity.bundleIdentifier, version: AppVersion("0.2.0")!)
        try await install()
        #expect(fake.calls == ["download checksum", "download zip", "extract", "trust", "runningIdentity", "replace", "runningIdentity"])
        #expect(try leftoverWork().isEmpty)
    }

    @Test func timeoutWithoutReplaceKeepsStagedApp() async throws {
        fake.reply = .timedOut
        await expectFailure(.rejected("The update didn't finish. If Dictator was replaced, quit it and open it again."))
        let work = try leftoverWork()
        #expect(work.count == 1)
        let staged = workRoot.appending(path: work[0]).appending(path: "unzipped/Dictator.app")
        #expect(FileManager.default.fileExists(atPath: staged.path))
    }
}
