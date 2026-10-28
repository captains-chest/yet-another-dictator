import CryptoKit
import Foundation
import Testing
@testable import WhisperTranscription

/// Counts fetches and can hold them until released, to prove concurrent calls share one download.
final class CountingTransport: ModelTransport, @unchecked Sendable {
    let inner: FakeTransport
    private let lock = NSLock()
    private var count = 0
    private var gate: CheckedContinuation<Void, Never>?
    private var waitsForGate: Bool

    init(files: [String: Data], holdFirstFetch: Bool = false) {
        inner = FakeTransport(files: files)
        waitsForGate = holdFirstFetch
    }

    var fetches: Int { lock.withLock { count } }

    func release() {
        let gate = lock.withLock { () -> CheckedContinuation<Void, Never>? in
            waitsForGate = false
            defer { self.gate = nil }
            return self.gate
        }
        gate?.resume()
    }

    var isHeld: Bool { lock.withLock { gate != nil } }

    func fetch(_ entry: ModelManifest.Entry, into part: URL, progress: @escaping @Sendable (Int64) -> Void) async throws {
        let hold = lock.withLock { () -> Bool in
            count += 1
            return waitsForGate
        }
        if hold {
            await withCheckedContinuation { continuation in
                let resumeNow = lock.withLock { () -> Bool in
                    if !waitsForGate { return true }
                    gate = continuation
                    return false
                }
                if resumeNow { continuation.resume() }
            }
        }
        try await inner.fetch(entry, into: part, progress: progress)
    }
}

/// Collects progress callbacks from any thread.
final class ProgressLog: @unchecked Sendable {
    private let lock = NSLock()
    private var items: [ModelInstaller.Progress] = []
    func append(_ progress: ModelInstaller.Progress) { lock.withLock { items.append(progress) } }
    var all: [ModelInstaller.Progress] { lock.withLock { items } }
}

struct ModelProvisionerTests {
    let files = ["a.bin": Data(repeating: 7, count: 1000), "dir/b.bin": Data((0..<300).map { UInt8($0 % 256) })]

    func installer() throws -> ModelInstaller {
        let lines = files.sorted { $0.key < $1.key }.map { path, data in
            "m  \(SHA256.hash(data: data).hex)  \(data.count)  org/r/resolve/c/\(path)  \(path)"
        }
        var installer = ModelInstaller(
            manifest: try ModelManifest(text: lines.joined(separator: "\n") + "\n"),
            modelID: "m",
            root: FileManager.default.temporaryDirectory.appending(path: UUID().uuidString, directoryHint: .isDirectory)
        )
        installer.maxAttempts = 1
        return installer
    }

    @Test func alreadyInstalledShortCircuitsWithoutFetching() async throws {
        let installer = try installer()
        try await installer.install(using: FakeTransport(files: files))
        let transport = CountingTransport(files: files)
        let provisioner = ModelProvisioner(installer: installer, transport: transport)

        #expect(await provisioner.ensureInstalled())
        #expect(await provisioner.status == .ready)
        #expect(transport.fetches == 0)
    }

    @Test func failureIsReportedThenRetrySucceeds() async throws {
        let transport = FakeTransport(files: files)
        transport.failOnceAfter = 100
        let provisioner = ModelProvisioner(installer: try installer(), transport: transport)

        #expect(await provisioner.ensureInstalled() == false)
        #expect(await provisioner.status == .failed("No internet connection. Dictator retries when you choose Retry."))

        #expect(await provisioner.ensureInstalled())
        #expect(await provisioner.status == .ready)
        #expect(transport.resumedFrom.contains(100))
    }

    @Test func reportsProgressUpToTheTotal() async throws {
        let log = ProgressLog()
        let installer = try installer()
        let provisioner = ModelProvisioner(installer: installer, transport: FakeTransport(files: files)) { log.append($0) }

        #expect(await provisioner.ensureInstalled())
        let progress = log.all
        #expect(!progress.isEmpty)
        #expect(progress.last == ModelInstaller.Progress(completedBytes: installer.totalBytes, totalBytes: installer.totalBytes))
        #expect(zip(progress, progress.dropFirst()).allSatisfy { $0.completedBytes <= $1.completedBytes })
    }

    @Test func concurrentCallsShareOneDownload() async throws {
        let transport = CountingTransport(files: files, holdFirstFetch: true)
        let provisioner = ModelProvisioner(installer: try installer(), transport: transport)

        async let first = provisioner.ensureInstalled()
        // Wait until the first download is in flight, then ask again.
        while !transport.isHeld { try await Task.sleep(for: .milliseconds(5)) }
        if case .downloading = await provisioner.status {} else { Issue.record("expected downloading") }
        async let second = provisioner.ensureInstalled()
        try await Task.sleep(for: .milliseconds(50))
        transport.release()

        let results = await [first, second]
        #expect(results == [true, true])
        #expect(transport.fetches == files.count)
        #expect(await provisioner.status == .ready)
    }

    @Test func mapsErrorsToUserFacingText() {
        #expect(ModelProvisioner.describe(URLError(.notConnectedToInternet)) == "No internet connection. Dictator retries when you choose Retry.")
        #expect(ModelProvisioner.describe(CocoaError(.fileWriteOutOfSpace)) == "Not enough disk space (about 1.7 GB needed).")
    }
}
