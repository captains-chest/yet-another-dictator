import CryptoKit
import Foundation

/// Fetches one manifest file's bytes into a `.part` file, resuming from what is already there.
/// The installer verifies the result; a transport only moves bytes.
public protocol ModelTransport: Sendable {
    /// Appends the missing bytes of `entry` to `part`, which may already hold a prefix. Calls
    /// `progress` with the number of bytes in `part` so far.
    func fetch(_ entry: ModelManifest.Entry, into part: URL, progress: @escaping @Sendable (Int64) -> Void) async throws
}

/// Downloads a model listed in the bundled manifest into `<root>/<model-id>/`, verifying every file's
/// SHA-256 before it is moved into place. Resumes from `.part` files and retries with backoff.
/// A `.complete` marker, written last, tells the loader the folder is whole.
public struct ModelInstaller: Sendable {
    public enum Failure: Error, CustomStringConvertible {
        case checksumMismatch(String)
        case unknownModel(String)

        public var description: String {
            switch self {
            case .checksumMismatch(let file): "Downloaded file failed verification: \(file)"
            case .unknownModel(let id): "Model \(id) is not in the manifest"
            }
        }
    }

    public struct Progress: Sendable, Equatable {
        public let completedBytes: Int64
        public let totalBytes: Int64
        public var fraction: Double { totalBytes > 0 ? Double(completedBytes) / Double(totalBytes) : 0 }

        public init(completedBytes: Int64, totalBytes: Int64) {
            self.completedBytes = completedBytes
            self.totalBytes = totalBytes
        }
    }

    public let manifest: ModelManifest
    public let modelID: String
    public let root: URL
    public var maxAttempts = 5

    public init(manifest: ModelManifest, modelID: String, root: URL) {
        self.manifest = manifest
        self.modelID = modelID
        self.root = root
    }

    /// `~/Library/Application Support/<app>/Models` (inside the sandbox container on macOS).
    public static func defaultRoot() throws -> URL {
        let support = try FileManager.default.url(
            for: .applicationSupportDirectory, in: .userDomainMask, appropriateFor: nil, create: true
        )
        return support.appending(path: "Models", directoryHint: .isDirectory)
    }

    public var folder: URL { root.appending(path: modelID, directoryHint: .isDirectory) }
    private var marker: URL { folder.appending(path: ".complete") }

    public var totalBytes: Int64 { manifest.totalBytes(for: modelID) }

    /// The marker holds a digest of this model's manifest lines, so a model bump means re-download.
    private var manifestDigest: String {
        let lines = manifest.files(for: modelID).map { "\($0.sha256) \($0.localPath)" }.joined(separator: "\n")
        return SHA256.hash(data: Data(lines.utf8)).hex
    }

    public var isInstalled: Bool {
        (try? String(contentsOf: marker, encoding: .utf8)) == manifestDigest
    }

    /// Downloads whatever is missing. Safe to call again after a failure or a quit: it resumes.
    public func install(
        using transport: some ModelTransport,
        progress: @escaping @Sendable (Progress) -> Void = { _ in }
    ) async throws {
        let files = manifest.files(for: modelID)
        guard !files.isEmpty else { throw Failure.unknownModel(modelID) }
        if isInstalled { return }
        try removeStaleModels()
        let fm = FileManager.default
        try fm.createDirectory(at: folder, withIntermediateDirectories: true)
        try? fm.removeItem(at: marker)
        var excluded = URLResourceValues()
        excluded.isExcludedFromBackup = true
        var rootURL = root
        try? rootURL.setResourceValues(excluded)

        let total = totalBytes
        var done: Int64 = 0
        for entry in files {
            let target = folder.appending(path: entry.localPath)
            if try !isVerified(target, entry) {
                let base = done
                try await fetchWithRetry(entry, to: target, using: transport) { written in
                    progress(Progress(completedBytes: base + written, totalBytes: total))
                }
            }
            done += entry.bytes
            progress(Progress(completedBytes: done, totalBytes: total))
        }
        try Data(manifestDigest.utf8).write(to: marker, options: .atomic)
    }

    /// Deletes this model's folder (the user switched models and chose to free the space).
    public func uninstall() throws {
        let fm = FileManager.default
        guard fm.fileExists(atPath: folder.path(percentEncoded: false)) else { return }
        try fm.removeItem(at: folder)
    }

    /// Deletes model folders that are no longer in the manifest, and stray files in this one.
    public func removeStaleModels() throws {
        let fm = FileManager.default
        let keep = Set(manifest.modelIDs)
        for url in (try? fm.contentsOfDirectory(at: root, includingPropertiesForKeys: nil)) ?? []
        where !keep.contains(url.lastPathComponent) {
            try? fm.removeItem(at: url)
        }
    }

    private func fetchWithRetry(
        _ entry: ModelManifest.Entry, to target: URL, using transport: some ModelTransport,
        progress: @escaping @Sendable (Int64) -> Void
    ) async throws {
        let fm = FileManager.default
        try fm.createDirectory(at: target.deletingLastPathComponent(), withIntermediateDirectories: true)
        let part = target.appendingPathExtension("part")
        var attempt = 0
        while true {
            attempt += 1
            do {
                try Task.checkCancellation()
                try await transport.fetch(entry, into: part, progress: progress)
                guard try Self.size(of: part) == entry.bytes, try Self.sha256(of: part) == entry.sha256 else {
                    try? fm.removeItem(at: part)
                    throw Failure.checksumMismatch(entry.localPath)
                }
                try? fm.removeItem(at: target)
                try fm.moveItem(at: part, to: target)
                return
            } catch is CancellationError {
                throw CancellationError()
            } catch {
                if attempt >= maxAttempts { throw error }
                // 2, 4, 8, 16 s
                try await Task.sleep(for: .seconds(1 << attempt))
            }
        }
    }

    /// A file already in place counts only if size and hash match the manifest.
    private func isVerified(_ url: URL, _ entry: ModelManifest.Entry) throws -> Bool {
        guard FileManager.default.fileExists(atPath: url.path(percentEncoded: false)),
              (try? Self.size(of: url)) == entry.bytes
        else { return false }
        return try Self.sha256(of: url) == entry.sha256
    }

    static func size(of url: URL) throws -> Int64 {
        let attributes = try FileManager.default.attributesOfItem(atPath: url.path(percentEncoded: false))
        return (attributes[.size] as? NSNumber)?.int64Value ?? 0
    }

    static func sha256(of url: URL) throws -> String {
        let handle = try FileHandle(forReadingFrom: url)
        defer { try? handle.close() }
        var hasher = SHA256()
        while let chunk = try handle.read(upToCount: 4 << 20), !chunk.isEmpty {
            hasher.update(data: chunk)
        }
        return hasher.finalize().hex
    }
}

/// Foreground transport: HTTP Range requests in fixed-size chunks, appended to the `.part` file.
/// Resumes after a failure or a relaunch from the bytes already on disk.
public struct RangedTransport: ModelTransport {
    public let session: URLSession
    public let chunkBytes: Int64

    public init(session: URLSession = URLSession(configuration: .ephemeral), chunkBytes: Int64 = 8 << 20) {
        self.session = session
        self.chunkBytes = chunkBytes
    }

    public func fetch(_ entry: ModelManifest.Entry, into part: URL, progress: @escaping @Sendable (Int64) -> Void) async throws {
        let fm = FileManager.default
        if !fm.fileExists(atPath: part.path(percentEncoded: false)) { _ = fm.createFile(atPath: part.path(percentEncoded: false), contents: nil) }
        var offset = try ModelInstaller.size(of: part)
        if offset > entry.bytes {
            try fm.removeItem(at: part)
            _ = fm.createFile(atPath: part.path(percentEncoded: false), contents: nil)
            offset = 0
        }
        let handle = try FileHandle(forWritingTo: part)
        defer { try? handle.close() }
        try handle.seek(toOffset: UInt64(offset))
        while offset < entry.bytes {
            try Task.checkCancellation()
            let end = min(offset + chunkBytes, entry.bytes) - 1
            var request = URLRequest(url: entry.url, timeoutInterval: 60)
            request.setValue("bytes=\(offset)-\(end)", forHTTPHeaderField: "Range")
            let (data, response) = try await session.data(for: request)
            let status = (response as? HTTPURLResponse)?.statusCode ?? 0
            switch status {
            case 206:
                guard Int64(data.count) == end - offset + 1 else { throw URLError(.cannotParseResponse) }
            case 200 where offset == 0 && Int64(data.count) == entry.bytes:
                break // Small file served whole.
            default:
                throw URLError(.badServerResponse)
            }
            try handle.write(contentsOf: data)
            offset += Int64(data.count)
            progress(offset)
        }
    }
}

extension Digest {
    var hex: String { map { String(format: "%02x", $0) }.joined() }
}
