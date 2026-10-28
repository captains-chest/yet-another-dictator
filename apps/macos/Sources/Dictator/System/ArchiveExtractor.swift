import AppUpdate
import Foundation

enum UpdateInstallError: Error, Equatable {
    case download
    case checksum
    case archive
    case translocated
    case rejected(String)

    var message: String {
        switch self {
        case .download: "Couldn't download the update. Check the connection and try again."
        case .checksum: "The download didn't match its published checksum."
        case .archive: "The download wasn't a Dictator app."
        case .translocated: "Move Dictator to the Applications folder and open it from there, then try the update again."
        case .rejected(let message): message
        }
    }
}

/// Unpacks the release zip the same way the release workflow does (`ditto`), after refusing
/// archive entries that could write outside the destination.
enum ArchiveExtractor {
    static func extractApp(from zip: URL, into directory: URL) throws -> URL {
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        guard listingIsSafe(zip) else { throw UpdateInstallError.archive }
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/usr/bin/ditto")
        process.arguments = ["-x", "-k", "--", zip.path, directory.path]
        process.standardOutput = FileHandle.nullDevice
        process.standardError = FileHandle.nullDevice
        try process.run()
        process.waitUntilExit()
        guard process.terminationStatus == 0 else { throw UpdateInstallError.archive }
        let app = directory.appending(path: "Dictator.app", directoryHint: .isDirectory)
        let root = directory.resolvingSymlinksInPath().standardizedFileURL.path
        let unpacked = app.resolvingSymlinksInPath().standardizedFileURL
        guard unpacked.path.hasPrefix(root + "/"), unpacked.lastPathComponent == "Dictator.app" else {
            throw UpdateInstallError.archive
        }
        if (try? unpacked.resourceValues(forKeys: [.isSymbolicLinkKey]))?.isSymbolicLink == true {
            throw UpdateInstallError.archive
        }
        var isDirectory: ObjCBool = false
        guard FileManager.default.fileExists(atPath: unpacked.path, isDirectory: &isDirectory), isDirectory.boolValue else {
            throw UpdateInstallError.archive
        }
        return unpacked
    }

    private static func listingIsSafe(_ zip: URL) -> Bool {
        let list = FileManager.default.temporaryDirectory.appending(path: "dictator-zip-list-\(UUID().uuidString).txt")
        FileManager.default.createFile(atPath: list.path, contents: nil)
        defer { try? FileManager.default.removeItem(at: list) }
        guard let output = try? FileHandle(forWritingTo: list) else { return false }
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/usr/bin/unzip")
        process.arguments = ["-Z1", "--", zip.path]
        process.standardOutput = output
        process.standardError = FileHandle.nullDevice
        guard (try? process.run()) != nil else { return false }
        process.waitUntilExit()
        try? output.close()
        guard process.terminationStatus == 0 else { return false }
        guard let bytes = try? list.resourceValues(forKeys: [.fileSizeKey]).fileSize, bytes <= ArchiveLimits.maxListingBytes else {
            return false
        }
        guard let text = try? String(contentsOf: list, encoding: .utf8) else { return false }
        var sawPlist = false
        for line in text.split(whereSeparator: \.isNewline) {
            let entry = String(line)
            if entry.hasPrefix("/") || entry.contains("\\") { return false }
            // `unzip` lists directories with a trailing slash. An empty component elsewhere is `//`.
            let trimmed = entry.hasSuffix("/") ? String(entry.dropLast()) : entry
            if trimmed.isEmpty { return false }
            let parts = trimmed.split(separator: "/", omittingEmptySubsequences: false)
            if parts.contains("..") || parts.contains(".") || parts.contains("") { return false }
            if trimmed == "Dictator.app/Contents/Info.plist" { sawPlist = true }
        }
        return sawPlist
    }
}
