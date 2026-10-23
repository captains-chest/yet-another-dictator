import CryptoKit
import Foundation
import Testing
@testable import WhisperTranscription

struct ModelManifestTests {
    let sha = String(repeating: "a", count: 64)

    @Test func parsesEntriesPerModel() throws {
        let manifest = try ModelManifest(text: """
        # comment
        small  \(sha)  10  org/repo/resolve/abc/a.bin  a.bin

        small  \(sha)  5  org/repo/resolve/abc/dir/b.bin  dir/b.bin
        dansk  \(sha)  7  org/other/resolve/def/c.bin  c.bin
        """)
        #expect(manifest.modelIDs == ["small", "dansk"])
        #expect(manifest.totalBytes(for: "small") == 15)
        #expect(manifest.files(for: "dansk").first?.url.absoluteString == "https://huggingface.co/org/other/resolve/def/c.bin")
    }

    @Test(arguments: ["m  SHA  1  r/x  ../x", "m  SHA  1  r/x  /x", "m  nothex  1  r/x  x", "m  SHA  1  r/x", "m  SHA  0  r/x  x"])
    func rejectsBadLines(_ line: String) {
        #expect(throws: ModelManifest.ParseError.self) {
            try ModelManifest(text: line.replacingOccurrences(of: "SHA", with: sha))
        }
    }

    @Test func theShippedManifestParses() throws {
        let url = URL(filePath: #filePath).deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
            .appending(path: "scripts/model-manifest.txt")
        let manifest = try ModelManifest(text: String(contentsOf: url, encoding: .utf8))
        #expect(manifest.modelIDs == ["openai_whisper-large-v3-v20240930_turbo", "parakeet-tdt-0.6b-v2", "s1-mini"])
        #expect(manifest.files(for: "openai_whisper-large-v3-v20240930_turbo").contains { $0.localPath == "tokenizer.json" })
        #expect(manifest.files(for: "parakeet-tdt-0.6b-v2").contains { $0.localPath == "parakeet_vocab.json" })
    }

    /// S1-mini is pinned to one immutable commit, with the hash and size of the Q4_K_M GGUF, and
    /// downloads its LICENSE and NOTICE with it (ADR 0011).
    @Test func theShippedManifestPinsS1Mini() throws {
        let url = URL(filePath: #filePath).deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
            .appending(path: "scripts/model-manifest.txt")
        let manifest = try ModelManifest(text: String(contentsOf: url, encoding: .utf8))
        let files = manifest.files(for: "s1-mini")
        let revision = "superwhisper/s1-mini-GGUF/resolve/34add00a48a2e5d24e5a4ee5405a99620a3a240c/"

        #expect(files.map(\.localPath) == ["s1-mini-q4_k_m.gguf", "LICENSE", "NOTICE"])
        #expect(files.allSatisfy { $0.remotePath.hasPrefix(revision) })
        let gguf = try #require(files.first)
        #expect(gguf.sha256 == "3b41ebe2502cbd03e811d5d16b022f5ab551eda58d62597d152f89535003c634")
        #expect(gguf.bytes == 484_219_808)
        #expect(gguf.url.absoluteString == "https://huggingface.co/\(revision)s1-mini-q4_k_m.gguf")
    }
}

/// Serves in-memory files, optionally failing once mid-way or corrupting a byte.
final class FakeTransport: ModelTransport, @unchecked Sendable {
    let files: [String: Data]
    var failOnceAfter: Int?
    var corrupt = false
    private(set) var resumedFrom: [Int64] = []
    private let lock = NSLock()

    init(files: [String: Data]) { self.files = files }

    func fetch(_ entry: ModelManifest.Entry, into part: URL, progress: @escaping @Sendable (Int64) -> Void) async throws {
        var data = files[entry.localPath]!
        if corrupt { data[0] ^= 0xFF }
        let existing = (try? Data(contentsOf: part)) ?? Data()
        lock.withLock { resumedFrom.append(Int64(existing.count)) }
        var rest = data.dropFirst(existing.count)
        if let cut = lock.withLock({ () -> Int? in defer { failOnceAfter = nil }; return failOnceAfter }) {
            try (existing + rest.prefix(cut)).write(to: part)
            throw URLError(.networkConnectionLost)
        }
        rest = data.dropFirst(existing.count)
        try (existing + rest).write(to: part)
        progress(Int64(data.count))
    }
}

struct ModelInstallerTests {
    let files = ["a.bin": Data(repeating: 1, count: 1000), "dir/b.bin": Data((0..<300).map { UInt8($0 % 256) })]

    func manifest() throws -> ModelManifest {
        let lines = files.sorted { $0.key < $1.key }.map { path, data in
            "m  \(SHA256.hash(data: data).hex)  \(data.count)  org/r/resolve/c/\(path)  \(path)"
        }
        return try ModelManifest(text: lines.joined(separator: "\n") + "\n")
    }

    func tempRoot() -> URL {
        FileManager.default.temporaryDirectory.appending(path: UUID().uuidString, directoryHint: .isDirectory)
    }

    @Test func installsVerifiesAndMarksComplete() async throws {
        var installer = ModelInstaller(manifest: try manifest(), modelID: "m", root: tempRoot())
        installer.maxAttempts = 1
        #expect(!installer.isInstalled)
        try await installer.install(using: FakeTransport(files: files))
        #expect(installer.isInstalled)
        #expect(try Data(contentsOf: installer.folder.appending(path: "dir/b.bin")) == files["dir/b.bin"])
    }

    @Test func resumesAfterADroppedConnection() async throws {
        var installer = ModelInstaller(manifest: try manifest(), modelID: "m", root: tempRoot())
        installer.maxAttempts = 1
        let transport = FakeTransport(files: files)
        transport.failOnceAfter = 400
        await #expect(throws: URLError.self) { try await installer.install(using: transport) }
        #expect(!installer.isInstalled)
        try await installer.install(using: transport)
        #expect(installer.isInstalled)
        #expect(transport.resumedFrom.contains(400))
    }

    @Test func rejectsACorruptDownload() async throws {
        var installer = ModelInstaller(manifest: try manifest(), modelID: "m", root: tempRoot())
        installer.maxAttempts = 1
        let transport = FakeTransport(files: files)
        transport.corrupt = true
        await #expect(throws: ModelInstaller.Failure.self) { try await installer.install(using: transport) }
        #expect(!installer.isInstalled)
        #expect(!FileManager.default.fileExists(atPath: installer.folder.appending(path: "a.bin").path(percentEncoded: false)))
    }

    @Test func removesModelsNoLongerInTheManifest() async throws {
        let root = tempRoot()
        let stale = root.appending(path: "old-model", directoryHint: .isDirectory)
        try FileManager.default.createDirectory(at: stale, withIntermediateDirectories: true)
        var installer = ModelInstaller(manifest: try manifest(), modelID: "m", root: root)
        installer.maxAttempts = 1
        try await installer.install(using: FakeTransport(files: files))
        #expect(!FileManager.default.fileExists(atPath: stale.path(percentEncoded: false)))
    }
}
