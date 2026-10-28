import Foundation

/// The pinned list of model files the app may download. It ships inside the signed app and is the
/// only trust anchor: it is never fetched from the network.
///
/// Line format: `<model-id>  <sha256>  <bytes>  <huggingface-path>  <local-path>`; `#` starts a comment.
public struct ModelManifest: Sendable, Equatable {
    public struct Entry: Sendable, Equatable {
        public let modelID: String
        public let sha256: String
        public let bytes: Int64
        public let remotePath: String
        public let localPath: String

        public var url: URL { URL(string: "https://huggingface.co/\(remotePath)")! }
    }

    public enum ParseError: Error, Equatable {
        case malformed(line: String)
    }

    public let entries: [Entry]

    public init(text: String) throws {
        var entries: [Entry] = []
        for raw in text.split(whereSeparator: \.isNewline) {
            let line = raw.trimmingCharacters(in: .whitespaces)
            if line.isEmpty || line.hasPrefix("#") { continue }
            let parts = line.split(whereSeparator: \.isWhitespace).map(String.init)
            guard parts.count == 5,
                  parts[1].count == 64, parts[1].allSatisfy({ $0.isHexDigit && !$0.isUppercase }),
                  let bytes = Int64(parts[2]), bytes > 0,
                  Self.isSafe(parts[3]), Self.isSafe(parts[4]),
                  URL(string: "https://huggingface.co/\(parts[3])") != nil
            else { throw ParseError.malformed(line: line) }
            entries.append(Entry(modelID: parts[0], sha256: parts[1], bytes: bytes, remotePath: parts[3], localPath: parts[4]))
        }
        guard Set(entries.map { "\($0.modelID)/\($0.localPath)" }).count == entries.count else {
            throw ParseError.malformed(line: "duplicate entry")
        }
        self.entries = entries
    }

    /// The manifest bundled in the app (`model-manifest.txt` in the app's resources).
    public static func bundled(in bundle: Bundle = .main) throws -> ModelManifest {
        guard let url = bundle.url(forResource: "model-manifest", withExtension: "txt") else {
            throw CocoaError(.fileNoSuchFile)
        }
        return try ModelManifest(text: String(contentsOf: url, encoding: .utf8))
    }

    public var modelIDs: [String] {
        var seen = Set<String>()
        return entries.map(\.modelID).filter { seen.insert($0).inserted }
    }

    public func files(for modelID: String) -> [Entry] { entries.filter { $0.modelID == modelID } }

    public func totalBytes(for modelID: String) -> Int64 { files(for: modelID).reduce(0) { $0 + $1.bytes } }

    private static func isSafe(_ path: String) -> Bool {
        !path.isEmpty && !path.hasPrefix("/")
            && !path.split(separator: "/", omittingEmptySubsequences: false).contains { $0 == ".." || $0.isEmpty }
            && path.allSatisfy { $0.isASCII && ($0.isLetter || $0.isNumber || "._-/".contains($0)) }
    }
}
