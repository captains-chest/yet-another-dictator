import CryptoKit
import DictationCore
import Foundation

/// How big an update is allowed to be. The app archive is a few megabytes; the cap is so a bad
/// release asset cannot fill the disk before the signature check rejects it.
public enum ArchiveLimits {
    public static let maxChecksumBytes = 4_096
    public static let maxArchiveBytes = 250 * 1_024 * 1_024
    public static let maxListingBytes = 1_024 * 1_024
}

/// Hosts a release download is allowed to redirect to. The first URL is always `github.com`.
public enum UpdateDownloadURL {
    public static let allowedHosts: Set<String> = [
        "github.com",
        "release-assets.githubusercontent.com",
        "objects.githubusercontent.com",
    ]

    public static func isAllowed(_ url: URL) -> Bool {
        guard url.scheme == "https", url.user == nil, let host = url.host?.lowercased() else { return false }
        return allowedHosts.contains(host)
    }
}

enum ReleaseAssetURL {
    /// The browser download URL GitHub puts on a release asset of this repository.
    static func isOfficialDownload(_ url: URL) -> Bool {
        guard UpdateDownloadURL.isAllowed(url), url.host?.lowercased() == "github.com" else { return false }
        let parts = url.path.split(separator: "/").map(String.init)
        guard parts.count == 6 else { return false }
        return parts[0] == "captains-chest"
            && parts[1] == "yet-another-dictator"
            && parts[2] == "releases"
            && parts[3] == "download"
            && !parts.contains("..")
            && !parts.contains(".")
    }
}

public struct GitHubAsset: Decodable, Equatable, Sendable {
    public var name: String
    public var downloadURL: URL?
    public var size: Int

    public init(name: String, downloadURL: URL?, size: Int = 0) {
        self.name = name
        self.downloadURL = downloadURL
        self.size = size
    }

    enum CodingKeys: String, CodingKey {
        case name
        case browserDownloadURL = "browser_download_url"
        case size
    }

    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        name = try container.decode(String.self, forKey: .name)
        size = try container.decodeIfPresent(Int.self, forKey: .size) ?? 0
        let raw = try container.decodeIfPresent(String.self, forKey: .browserDownloadURL) ?? ""
        downloadURL = URL(string: raw)
    }
}

public struct GitHubRelease: Decodable, Equatable, Sendable {
    public var tagName: String
    public var htmlURL: String
    public var draft: Bool
    public var prerelease: Bool
    public var assets: [GitHubAsset]

    public init(tagName: String, htmlURL: String, draft: Bool = false, prerelease: Bool = false, assets: [GitHubAsset] = []) {
        self.tagName = tagName
        self.htmlURL = htmlURL
        self.draft = draft
        self.prerelease = prerelease
        self.assets = assets
    }

    enum CodingKeys: String, CodingKey {
        case tagName = "tag_name"
        case htmlURL = "html_url"
        case draft, prerelease, assets
    }

    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        tagName = try container.decode(String.self, forKey: .tagName)
        htmlURL = try container.decode(String.self, forKey: .htmlURL)
        draft = try container.decodeIfPresent(Bool.self, forKey: .draft) ?? false
        prerelease = try container.decodeIfPresent(Bool.self, forKey: .prerelease) ?? false
        assets = try container.decodeIfPresent([GitHubAsset].self, forKey: .assets) ?? []
    }

    var releaseInfo: ReleaseInfo {
        ReleaseInfo(tagName: tagName, htmlURL: htmlURL, draft: draft, prerelease: prerelease)
    }
}

/// A newer release. `canInstall` is true when the release has this app's zip and its `.sha256`
/// on the official download URL. Otherwise the menu can still open the release page.
public struct OfferedUpdate: Equatable, Sendable {
    public let version: AppVersion
    public let pageURL: URL
    public let zipURL: URL?
    public let checksumURL: URL?
    public let zipBytes: Int?

    public var canInstall: Bool { zipURL != nil && checksumURL != nil }

    public init(version: AppVersion, pageURL: URL, zipURL: URL?, checksumURL: URL?, zipBytes: Int?) {
        self.version = version
        self.pageURL = pageURL
        self.zipURL = zipURL
        self.checksumURL = checksumURL
        self.zipBytes = zipBytes
    }

    public static func newest(in releases: [GitHubRelease], current: AppVersion) -> OfferedUpdate? {
        guard let best = UpdateCheck.newest(in: releases.map(\.releaseInfo), current: current) else { return nil }
        guard let release = releases.first(where: { !$0.draft && AppVersion($0.tagName) == best.version }) else {
            return OfferedUpdate(version: best.version, pageURL: best.pageURL, zipURL: nil, checksumURL: nil, zipBytes: nil)
        }
        let zipName = DictatorIdentity.archiveName(tag: release.tagName)
        let zipAsset = release.assets.first { $0.name == zipName }
        let checksumAsset = release.assets.first { $0.name == zipName + ".sha256" }
        let zipURL = zipAsset?.downloadURL.flatMap { ReleaseAssetURL.isOfficialDownload($0) ? $0 : nil }
        let checksumURL = checksumAsset?.downloadURL.flatMap { ReleaseAssetURL.isOfficialDownload($0) ? $0 : nil }
        let pair = zipURL != nil && checksumURL != nil
        return OfferedUpdate(
            version: best.version,
            pageURL: best.pageURL,
            zipURL: pair ? zipURL : nil,
            checksumURL: pair ? checksumURL : nil,
            zipBytes: pair ? zipAsset?.size : nil
        )
    }
}

/// A `shasum -a 256` file: `<64 hex><spaces><filename>`.
public enum ChecksumFile {
    public static func sha256(in text: String, forFileNamed fileName: String) -> String? {
        for line in text.split(whereSeparator: \.isNewline) {
            let trimmed = line.trimmingCharacters(in: .whitespaces)
            if trimmed.isEmpty || trimmed.hasPrefix("#") { continue }
            let parts = trimmed.split(separator: " ", omittingEmptySubsequences: true)
            guard parts.count >= 2 else { continue }
            let hash = String(parts[0]).lowercased()
            guard hash.count == 64, hash.allSatisfy(\.isHexDigit) else { continue }
            var name = parts.dropFirst().joined(separator: " ")
            if name.hasPrefix("*") { name.removeFirst() }
            if name == fileName { return hash }
        }
        return nil
    }
}

public enum FileDigest {
    public static func sha256Hex(of file: URL) throws -> String {
        let handle = try FileHandle(forReadingFrom: file)
        defer { try? handle.close() }
        var hasher = SHA256()
        while true {
            guard let chunk = try handle.read(upToCount: 1024 * 1024), !chunk.isEmpty else { break }
            hasher.update(data: chunk)
        }
        return hasher.finalize().map { String(format: "%02x", $0) }.joined()
    }
}

public enum ArchiveIntegrity {
    public static func matches(file: URL, checksumText: String, fileName: String) -> Bool {
        guard let expected = ChecksumFile.sha256(in: checksumText, forFileNamed: fileName) else { return false }
        guard let actual = try? FileDigest.sha256Hex(of: file) else { return false }
        return actual == expected
    }
}
