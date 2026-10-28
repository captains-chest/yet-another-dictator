import AppUpdate
import DictationCore
import Foundation
import Testing

@testable import Dictator

struct UpdateTests {
    private func release(
        _ tag: String,
        zip: String? = nil,
        checksum: String? = nil,
        draft: Bool = false,
        prerelease: Bool = false,
        zipSize: Int = 1200
    ) -> GitHubRelease {
        let page = "https://github.com/captains-chest/yet-another-dictator/releases/tag/\(tag)"
        var assets: [GitHubAsset] = []
        if let zip {
            assets.append(GitHubAsset(name: zip, downloadURL: URL(string: "https://github.com/captains-chest/yet-another-dictator/releases/download/\(tag)/\(zip)"), size: zipSize))
        }
        if let checksum {
            assets.append(GitHubAsset(
                name: checksum,
                downloadURL: URL(string: "https://github.com/captains-chest/yet-another-dictator/releases/download/\(tag)/\(checksum)"),
                size: 80
            ))
        }
        return GitHubRelease(tagName: tag, htmlURL: page, draft: draft, prerelease: prerelease, assets: assets)
    }

    @Test func newestReleaseKeepsTheZipAndChecksum() throws {
        let name = DictatorIdentity.archiveName(tag: "v0.2.0")
        let update = OfferedUpdate.newest(
            in: [release("v0.2.0", zip: name, checksum: name + ".sha256")],
            current: AppVersion("0.1.4")!
        )
        let offered = try #require(update)
        #expect(offered.version.description == "0.2.0")
        #expect(offered.canInstall)
        #expect(offered.zipBytes == 1200)
        #expect(offered.zipURL?.lastPathComponent == name)
    }

    @Test func releaseWithoutTheArchiveStillSurfacesThePage() throws {
        let update = try #require(OfferedUpdate.newest(
            in: [release("v0.2.0")],
            current: AppVersion("0.1.0")!
        ))
        #expect(!update.canInstall)
        #expect(update.pageURL.host == "github.com")
    }

    @Test func unofficialOrEscapingDownloadURLsAreNotInstalled() throws {
        let name = DictatorIdentity.archiveName(tag: "v0.2.0")
        let evil = GitHubRelease(
            tagName: "v0.2.0",
            htmlURL: "https://github.com/captains-chest/yet-another-dictator/releases/tag/v0.2.0",
            assets: [
                GitHubAsset(name: name, downloadURL: URL(string: "https://example.com/\(name)")),
                GitHubAsset(name: name + ".sha256", downloadURL: URL(string: "https://example.com/\(name).sha256")),
            ]
        )
        let update = try #require(OfferedUpdate.newest(in: [evil], current: AppVersion("0.1.0")!))
        #expect(!update.canInstall)

        let slipped = "https://github.com/captains-chest/yet-another-dictator/releases/download/v0.2.0/../../evil.zip"
        #expect(UpdateDownloadURL.isAllowed(URL(string: slipped)!))
        #expect(OfferedUpdate.newest(
            in: [GitHubRelease(
                tagName: "v0.2.0",
                htmlURL: "https://github.com/captains-chest/yet-another-dictator/releases/tag/v0.2.0",
                assets: [
                    GitHubAsset(name: name, downloadURL: URL(string: slipped)),
                    GitHubAsset(name: name + ".sha256", downloadURL: URL(string: "https://github.com/captains-chest/yet-another-dictator/releases/download/v0.2.0/\(name).sha256")),
                ]
            )],
            current: AppVersion("0.1.0")!
        )?.canInstall == false)
    }

    @Test func githubWireFormatDecodesTheArchive() throws {
        let json = """
        [{
          "tag_name": "v0.2.0",
          "html_url": "https://github.com/captains-chest/yet-another-dictator/releases/tag/v0.2.0",
          "draft": false,
          "prerelease": false,
          "assets": [{
            "name": "Dictator-v0.2.0-macos-arm64.zip",
            "browser_download_url": "https://github.com/captains-chest/yet-another-dictator/releases/download/v0.2.0/Dictator-v0.2.0-macos-arm64.zip",
            "size": 42,
            "content_type": "application/zip"
          }, {
            "name": "Dictator-v0.2.0-macos-arm64.zip.sha256",
            "browser_download_url": "https://github.com/captains-chest/yet-another-dictator/releases/download/v0.2.0/Dictator-v0.2.0-macos-arm64.zip.sha256",
            "size": 90
          }]
        }]
        """
        let releases = try JSONDecoder().decode([GitHubRelease].self, from: Data(json.utf8))
        #expect(OfferedUpdate.newest(in: releases, current: AppVersion("0.1.4")!)?.canInstall == true)
    }

    @Test func checksumFileMatchesShasumOutput() {
        let name = "Dictator-v0.1.4-macos-arm64.zip"
        let text = "cb10a0da40c3a2cc3e7c637fd94ea89bfddbe8b7fa0d92b478327674198b8527  \(name)\n"
        #expect(ChecksumFile.sha256(in: text, forFileNamed: name) == "cb10a0da40c3a2cc3e7c637fd94ea89bfddbe8b7fa0d92b478327674198b8527")
        #expect(ChecksumFile.sha256(in: text, forFileNamed: "other.zip") == nil)
        #expect(ChecksumFile.sha256(in: "abc  \(name)", forFileNamed: name) == nil)
    }

    @Test func fileDigestMatchesAKnownHash() throws {
        let file = FileManager.default.temporaryDirectory.appending(path: "dictator-hash-\(UUID().uuidString).txt")
        try Data("hello".utf8).write(to: file)
        defer { try? FileManager.default.removeItem(at: file) }
        #expect(try FileDigest.sha256Hex(of: file) == "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824")
        let checksum = "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824  \(file.lastPathComponent)\n"
        #expect(ArchiveIntegrity.matches(file: file, checksumText: checksum, fileName: file.lastPathComponent))
    }

    @Test func redirectHostsAreTheReleaseAssetHosts() {
        #expect(UpdateDownloadURL.isAllowed(URL(string: "https://release-assets.githubusercontent.com/asset")!))
        #expect(UpdateDownloadURL.isAllowed(URL(string: "https://objects.githubusercontent.com/asset")!))
        #expect(!UpdateDownloadURL.isAllowed(URL(string: "http://github.com/captains-chest/yet-another-dictator/releases/download/v1/a.zip")!))
        #expect(!UpdateDownloadURL.isAllowed(URL(string: "https://user@github.com/a")!))
        #expect(!UpdateDownloadURL.isAllowed(URL(string: "https://github.com.evil.test/a")!))
    }

    @Test func developerIDRequirementParses() {
        #expect(SignedApp.requirementParses())
    }

    @Test func policyAcceptsANewerNotarizedBuildOnly() {
        let current = AppIdentity(bundleIdentifier: DictatorIdentity.bundleIdentifier, version: AppVersion("0.1.4"))
        let staged = ReleaseTrust(
            identity: AppIdentity(bundleIdentifier: DictatorIdentity.bundleIdentifier, version: AppVersion("0.1.5")),
            teamIdentifier: DictatorIdentity.teamIdentifier,
            signatureValid: true,
            hardenedRuntime: true,
            stapledTicket: true
        )
        #expect(InstallPolicy.refusal(replacing: current, with: staged, gatekeeperAccepted: true) == nil)
        #expect(InstallPolicy.refusal(replacing: current, with: staged, gatekeeperAccepted: false) == InstallPolicy.untrusted)
        #expect(InstallPolicy.versionRefusal(
            replacing: current,
            with: AppIdentity(bundleIdentifier: DictatorIdentity.bundleIdentifier, version: AppVersion("0.1.4"))
        ) != nil)
        #expect(InstallPolicy.versionRefusal(
            replacing: AppIdentity(bundleIdentifier: "com.example.other", version: AppVersion("0.1.0")),
            with: staged.identity
        ) != nil)
        var unstapled = staged
        unstapled.stapledTicket = false
        #expect(InstallPolicy.signatureRefusal(unstapled) != nil)
    }

    @Test func unsignedBundleIsNotARelease() throws {
        let app = try fakeApp(named: "Unsigned.app", version: "9.0.0")
        defer { try? FileManager.default.removeItem(at: app.deletingLastPathComponent()) }
        let trust = SignedApp.trust(at: app)
        #expect(!trust.signatureValid)
        #expect(trust.identity.bundleIdentifier == DictatorIdentity.bundleIdentifier)
        #expect(trust.identity.version == AppVersion("9.0.0"))
    }

    @Test func replacerRefusesAnUnsignedAppAndLeavesTheOldOne() throws {
        let root = FileManager.default.temporaryDirectory.appending(path: "dictator-replace-\(UUID().uuidString)", directoryHint: .isDirectory)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        let destination = try fakeApp(named: "Dictator.app", version: "0.1.4", in: root)
        let staged = try fakeApp(named: "Staged.app", version: "0.2.0", in: root.appending(path: "staged", directoryHint: .isDirectory))
        let marker = destination.appending(path: "Contents/keep.txt")
        try Data("keep".utf8).write(to: marker)

        let message = AppReplacer.install(stagedApp: staged, replacing: destination)
        #expect(message == InstallPolicy.untrusted)
        #expect(try String(contentsOf: marker, encoding: .utf8) == "keep")
    }

    @Test func replacerIgnoresASymlinkAndAForeignPreviousCopy() throws {
        let root = FileManager.default.temporaryDirectory.appending(path: "dictator-link-\(UUID().uuidString)", directoryHint: .isDirectory)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        let destination = try fakeApp(named: "Dictator.app", version: "0.1.4", in: root)
        let staged = try fakeApp(named: "Staged.app", version: "0.2.0", in: root)
        let link = root.appending(path: "Link.app")
        try FileManager.default.createSymbolicLink(at: link, withDestinationURL: destination)
        #expect(AppReplacer.install(stagedApp: staged, replacing: link) != nil)
        #expect(FileManager.default.fileExists(atPath: destination.appending(path: "Contents/Info.plist").path))

        let foreign = root.appending(path: DictatorIdentity.previousCopyName)
        try FileManager.default.createDirectory(at: foreign.appending(path: "Contents"), withIntermediateDirectories: true)
        try plist(identifier: "com.example.other", version: "1.0.0").write(to: foreign.appending(path: "Contents/Info.plist"), atomically: true, encoding: .utf8)
        #expect(AppReplacer.removePreviousCopy(beside: destination) == nil)
        #expect(FileManager.default.fileExists(atPath: foreign.path))
    }

    @Test func runningFromThePreviousCopyKeepsIt() throws {
        let root = FileManager.default.temporaryDirectory.appending(path: "dictator-previous-\(UUID().uuidString)", directoryHint: .isDirectory)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        let running = try fakeApp(named: DictatorIdentity.previousCopyName, version: "0.1.4", in: root)
        let staged = try fakeApp(named: "Staged.app", version: "0.2.0", in: root.appending(path: "staged", directoryHint: .isDirectory))
        let marker = running.appending(path: "Contents/keep.txt")
        try Data("keep".utf8).write(to: marker)

        #expect(AppReplacer.removePreviousCopy(beside: running) == nil)
        #expect(try String(contentsOf: marker, encoding: .utf8) == "keep")

        let message = AppReplacer.install(stagedApp: staged, replacing: running)
        #expect(message?.contains("Dictator.app") == true)
        #expect(try String(contentsOf: marker, encoding: .utf8) == "keep")
        #expect(AppIdentity.read(from: running).version == AppVersion("0.1.4"))
    }

    @Test func hostApplicationIsThreeLevelsAboveTheXPC() {
        let xpc = URL(fileURLWithPath: "/Applications/Dictator.app/Contents/XPCServices/DictatorUpdater.xpc")
        #expect(AppBundleLocation.hostApplication(containingXPC: xpc)?.path == "/Applications/Dictator.app")
        #expect(AppBundleLocation.hostApplication(containingXPC: URL(fileURLWithPath: "/tmp/Nope.xpc")) == nil)
    }

    @Test func translocationPathIsRecognized() {
        #expect(AppLocation.isTranslocated(path: "/private/var/folders/ab/AppTranslocation/ABC/d/Dictator.app"))
        #expect(!AppLocation.isTranslocated(path: "/Applications/Dictator.app"))
    }

    @Test func extractRejectsEntriesThatEscapeTheArchive() throws {
        let root = FileManager.default.temporaryDirectory.appending(path: "dictator-zip-\(UUID().uuidString)", directoryHint: .isDirectory)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        let zip = root.appending(path: "bad.zip")
        let script = """
        import zipfile
        z = zipfile.ZipFile("\(zip.path)", "w")
        z.writestr("Dictator.app/Contents/Info.plist", "plist")
        z.writestr("../escaped.txt", "no")
        z.close()
        """
        let python = Process()
        python.executableURL = URL(fileURLWithPath: "/usr/bin/python3")
        python.arguments = ["-c", script]
        try python.run()
        python.waitUntilExit()
        #expect(python.terminationStatus == 0)
        #expect(throws: UpdateInstallError.archive) {
            try ArchiveExtractor.extractApp(from: zip, into: root.appending(path: "out", directoryHint: .isDirectory))
        }
    }

    /// Downloads the published 0.1.4 zip and replaces a throwaway older app. Opt-in so CI stays offline.
    @Test func publishedReleaseReplacesAnOlderCopy() async throws {
        guard ProcessInfo.processInfo.environment["DICTATOR_INSTALL_PROBE"] == "1" else { return }
        let name = DictatorIdentity.archiveName(tag: "v0.1.4")
        let base = "https://github.com/captains-chest/yet-another-dictator/releases/download/v0.1.4/"
        let work = FileManager.default.temporaryDirectory.appending(path: "dictator-probe-\(UUID().uuidString)", directoryHint: .isDirectory)
        try FileManager.default.createDirectory(at: work, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: work) }
        let zip = work.appending(path: name)
        let checksum = work.appending(path: "checksum.txt")
        try await UpdateDownloader.download(URL(string: base + name + ".sha256")!, to: checksum, limit: ArchiveLimits.maxChecksumBytes) { _ in }
        try await UpdateDownloader.download(URL(string: base + name)!, to: zip, limit: ArchiveLimits.maxArchiveBytes) { _ in }
        let text = try String(contentsOf: checksum, encoding: .utf8)
        #expect(ArchiveIntegrity.matches(file: zip, checksumText: text, fileName: name))
        let staged = try ArchiveExtractor.extractApp(from: zip, into: work.appending(path: "unzipped", directoryHint: .isDirectory))
        let destination = try fakeApp(named: "Dictator.app", version: "0.1.0", in: work.appending(path: "dest", directoryHint: .isDirectory))
        #expect(AppReplacer.install(stagedApp: staged, replacing: destination) == nil)
        #expect(AppIdentity.read(from: destination).version == AppVersion("0.1.4"))
        #expect(SignedApp.trust(at: destination).signatureValid)
        #expect(Gatekeeper.accepts(destination))
        let previous = destination.deletingLastPathComponent().appending(path: DictatorIdentity.previousCopyName)
        #expect(FileManager.default.fileExists(atPath: previous.path))
        #expect(AppReplacer.removePreviousCopy(beside: destination) == nil)
        #expect(!FileManager.default.fileExists(atPath: previous.path))
    }

    @Test func installedReleaseIsTrustedWhenPresent() {
        let app = URL(fileURLWithPath: "/Applications/Dictator.app")
        guard FileManager.default.fileExists(atPath: app.path) else { return }
        let trust = SignedApp.trust(at: app)
        #expect(trust.signatureValid)
        #expect(trust.hardenedRuntime)
        #expect(trust.stapledTicket)
        #expect(trust.teamIdentifier == DictatorIdentity.teamIdentifier)
        #expect(Gatekeeper.accepts(app))
    }

    private func fakeApp(named name: String, version: String, in directory: URL? = nil) throws -> URL {
        let root = directory ?? FileManager.default.temporaryDirectory.appending(path: "dictator-app-\(UUID().uuidString)", directoryHint: .isDirectory)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        let contents = root.appending(path: "\(name)/Contents", directoryHint: .isDirectory)
        try FileManager.default.createDirectory(at: contents, withIntermediateDirectories: true)
        try plist(identifier: DictatorIdentity.bundleIdentifier, version: version).write(to: contents.appending(path: "Info.plist"), atomically: true, encoding: .utf8)
        return root.appending(path: name, directoryHint: .isDirectory)
    }

    private func plist(identifier: String, version: String) -> String {
        """
        <?xml version="1.0" encoding="UTF-8"?>
        <!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
        <plist version="1.0"><dict>
        <key>CFBundleIdentifier</key><string>\(identifier)</string>
        <key>CFBundleShortVersionString</key><string>\(version)</string>
        </dict></plist>
        """
    }
}
