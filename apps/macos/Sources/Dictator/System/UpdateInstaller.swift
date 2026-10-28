import AppUpdate
import Foundation

enum UpdatePhase: Equatable, Sendable {
    case idle
    case downloading(Double)
    case checking
    case installing
    case failed(String)

    var allowsCheck: Bool {
        switch self {
        case .idle, .failed: true
        default: false
        }
    }
}

/// The side-effecting steps of an install, so the order and checks in `UpdateInstaller` can be
/// tested without the network, `ditto`, code signing or the updater.
protocol InstallSteps: Sendable {
    func download(_ url: URL, to destination: URL, limit: Int, progress: @escaping @Sendable (Double) -> Void) async throws
    func extractApp(from zip: URL, into directory: URL) throws -> URL
    func trust(at app: URL) -> ReleaseTrust
    func runningIdentity() -> AppIdentity
    func replace(stagedApp: URL) async -> UpdaterReply
}

struct LiveInstallSteps: InstallSteps {
    func download(_ url: URL, to destination: URL, limit: Int, progress: @escaping @Sendable (Double) -> Void) async throws {
        try await UpdateDownloader.download(url, to: destination, limit: limit, progress: progress)
    }

    func extractApp(from zip: URL, into directory: URL) throws -> URL {
        try ArchiveExtractor.extractApp(from: zip, into: directory)
    }

    func trust(at app: URL) -> ReleaseTrust {
        SignedApp.trust(at: app)
    }

    /// Read at call time: after a replace the bundle on disk may be the new version.
    func runningIdentity() -> AppIdentity {
        AppIdentity.read(from: Bundle.main.bundleURL)
    }

    func replace(stagedApp: URL) async -> UpdaterReply {
        await UpdateClient.install(stagedApp: stagedApp)
    }
}

/// Downloads one offered release, checks its checksum and signature, then asks the updater to
/// replace this app. Nothing is written over the running app until those checks pass.
struct UpdateInstaller: Sendable {
    private let steps: any InstallSteps
    private let workRoot: URL

    init(steps: any InstallSteps = LiveInstallSteps(), workRoot: URL = FileManager.default.temporaryDirectory) {
        self.steps = steps
        self.workRoot = workRoot
    }

    func install(_ update: OfferedUpdate, progress: @escaping @Sendable (UpdatePhase) -> Void) async throws {
        guard let zipURL = update.zipURL, let checksumURL = update.checksumURL else { throw UpdateInstallError.archive }
        let work = workRoot.appending(path: "dictator-update-\(UUID().uuidString)", directoryHint: .isDirectory)
        try FileManager.default.createDirectory(at: work, withIntermediateDirectories: true)
        var keepsWork = false
        defer { if !keepsWork { try? FileManager.default.removeItem(at: work) } }

        progress(.downloading(0))
        let checksumFile = work.appending(path: "checksum.txt")
        do {
            try await steps.download(checksumURL, to: checksumFile, limit: ArchiveLimits.maxChecksumBytes, progress: { _ in })
        } catch {
            throw UpdateInstallError.download
        }
        guard let checksumText = try? String(contentsOf: checksumFile, encoding: .utf8) else { throw UpdateInstallError.checksum }

        let zip = work.appending(path: zipURL.lastPathComponent)
        do {
            try await steps.download(zipURL, to: zip, limit: ArchiveLimits.maxArchiveBytes) { fraction in
                progress(.downloading(fraction))
            }
        } catch {
            throw UpdateInstallError.download
        }
        guard ArchiveIntegrity.matches(file: zip, checksumText: checksumText, fileName: zip.lastPathComponent) else {
            throw UpdateInstallError.checksum
        }

        progress(.checking)
        let app = try steps.extractApp(from: zip, into: work.appending(path: "unzipped", directoryHint: .isDirectory))
        let trust = steps.trust(at: app)
        guard let destination = currentIdentity() else { throw UpdateInstallError.rejected("The update's version couldn't be read.") }
        if let refusal = InstallPolicy.signatureRefusal(trust) ?? InstallPolicy.versionRefusal(replacing: destination, with: trust.identity) {
            throw UpdateInstallError.rejected(refusal)
        }

        progress(.installing)
        switch await steps.replace(stagedApp: app) {
        case .replied(nil):
            return
        case .replied(let message?):
            throw UpdateInstallError.rejected(message)
        case .timedOut:
            if steps.runningIdentity().version == update.version { return }
            // The updater may still be reading the staged app. The system clears temp files later.
            keepsWork = true
            throw UpdateInstallError.rejected("The update didn't finish. If Dictator was replaced, quit it and open it again.")
        }
    }

    private func currentIdentity() -> AppIdentity? {
        let identity = steps.runningIdentity()
        guard identity.bundleIdentifier != nil, identity.version != nil else { return nil }
        return identity
    }
}
