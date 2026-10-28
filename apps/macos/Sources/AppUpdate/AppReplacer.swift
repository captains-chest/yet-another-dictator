import Foundation

/// Replaces this app with a staged copy. Runs in the unsandboxed updater. Returns nil on success
/// and a short message the app can show when it refuses or fails.
public enum AppReplacer {
    public static func install(stagedApp: URL, replacing destination: URL) -> String? {
        guard let staged = realApp(stagedApp), let destination = realApp(destination), staged != destination else {
            return "The download wasn't a Dictator app."
        }
        // Replacing would delete the running copy first, since the backup lands on this path.
        let backup = destination.deletingLastPathComponent().appending(path: DictatorIdentity.previousCopyName)
        if destination.lastPathComponent == DictatorIdentity.previousCopyName || isSame(backup, destination) {
            return "This is the previous copy of Dictator. Quit it, open Dictator.app instead, and try again."
        }
        let destinationIdentity = AppIdentity.read(from: destination)
        let trust = SignedApp.trust(at: staged)
        if let refusal = InstallPolicy.signatureRefusal(trust)
            ?? InstallPolicy.versionRefusal(replacing: destinationIdentity, with: trust.identity) {
            return refusal
        }
        // Quarantine would make Gatekeeper assess the download as a browser file. The stapled
        // ticket still has to pass on its own once that marker is gone.
        stripQuarantine(staged)
        guard Gatekeeper.accepts(staged) else { return InstallPolicy.untrusted }

        if FileManager.default.fileExists(atPath: backup.path) {
            guard canDelete(backup) else {
                return "Move \(DictatorIdentity.previousCopyName) out of the way and try again."
            }
            do { try FileManager.default.removeItem(at: backup) } catch {
                return "Couldn't replace Dictator. Move it to the Applications folder and try again."
            }
        }
        // replaceItemAt only works on one volume. The download lives in a temp directory that
        // may not be the volume holding Dictator.app.
        let staging = VolumeStaging.prepare(staged, beside: destination)
        defer { staging.cleanup() }
        guard let ready = staging.ready else {
            return "Couldn't replace Dictator. Move it to the Applications folder and try again."
        }
        do {
            // The default options delete the backup after success. Keep it so a failed
            // post-check can restore, and so the next launch can remove it.
            _ = try FileManager.default.replaceItemAt(
                destination,
                withItemAt: ready,
                backupItemName: DictatorIdentity.previousCopyName,
                options: .withoutDeletingBackupItem
            )
        } catch {
            _ = recover(destinationIdentity, at: destination, backup: backup, error: error)
            return "Couldn't replace Dictator. Move it to the Applications folder and try again."
        }
        stripQuarantine(destination)
        let installed = SignedApp.trust(at: destination)
        if InstallPolicy.refusal(replacing: destinationIdentity, with: installed, gatekeeperAccepted: Gatekeeper.accepts(destination)) != nil {
            let restored = restore(backup: backup, to: destination)
            if restored {
                return "The update failed its signature check and was not kept."
            }
            return "The update was rejected and the previous copy couldn't be restored. Download Dictator again and move it to Applications."
        }
        return nil
    }

    /// Deletes `Dictator.previous.app` next to `app` when it is an old Dictator. Ignores anything else.
    public static func removePreviousCopy(beside app: URL) -> String? {
        guard let app = realApp(app) else { return nil }
        let previous = app.deletingLastPathComponent().appending(path: DictatorIdentity.previousCopyName)
        guard FileManager.default.fileExists(atPath: previous.path) else { return nil }
        // Running from the previous copy itself, e.g. opened from Spotlight.
        guard !isSame(previous, app), canDelete(previous) else { return nil }
        do {
            try FileManager.default.removeItem(at: previous)
            return nil
        } catch {
            return "Couldn't remove the previous copy of Dictator."
        }
    }

    private static func canDelete(_ url: URL) -> Bool {
        guard (try? url.resourceValues(forKeys: [.isSymbolicLinkKey]))?.isSymbolicLink != true else { return false }
        return AppIdentity.read(from: url).bundleIdentifier == DictatorIdentity.bundleIdentifier
    }

    /// Compares file identity too, so another spelling on a case-insensitive volume still matches.
    private static func isSame(_ left: URL, _ right: URL) -> Bool {
        if left.standardizedFileURL.resolvingSymlinksInPath().path == right.standardizedFileURL.resolvingSymlinksInPath().path {
            return true
        }
        let key = URLResourceKey.fileResourceIdentifierKey
        guard let a = try? left.resourceValues(forKeys: [key]).fileResourceIdentifier,
              let b = try? right.resourceValues(forKeys: [key]).fileResourceIdentifier
        else { return false }
        return a.isEqual(b)
    }

    /// A real `.app` directory, not a symlink. Intermediate symlinks (such as `/tmp`) are resolved
    /// so two paths to the same bundle compare equal.
    static func realApp(_ url: URL) -> URL? {
        if (try? url.resourceValues(forKeys: [.isSymbolicLinkKey]))?.isSymbolicLink == true { return nil }
        let resolved = url.standardizedFileURL.resolvingSymlinksInPath()
        if (try? resolved.resourceValues(forKeys: [.isSymbolicLinkKey]))?.isSymbolicLink == true { return nil }
        guard resolved.pathExtension == "app" else { return nil }
        var isDirectory: ObjCBool = false
        guard FileManager.default.fileExists(atPath: resolved.path, isDirectory: &isDirectory), isDirectory.boolValue else { return nil }
        guard FileManager.default.fileExists(atPath: resolved.appending(path: "Contents/Info.plist").path) else { return nil }
        return resolved
    }

    /// Puts the previous app back when a replace throws after moving it.
    private static func recover(_ original: AppIdentity, at destination: URL, backup: URL, error: Error) -> Bool {
        let current = AppIdentity.read(from: destination)
        if current.bundleIdentifier == original.bundleIdentifier, current.version == original.version {
            return true
        }
        if FileManager.default.fileExists(atPath: backup.path) {
            return restore(backup: backup, to: destination)
        }
        guard let located = (error as NSError).userInfo["NSFileOriginalItemLocationKey"] as? URL else { return false }
        return restore(backup: located, to: destination)
    }

    private static func restore(backup: URL, to destination: URL) -> Bool {
        let files = FileManager.default
        if files.fileExists(atPath: destination.path) {
            try? files.removeItem(at: destination)
        }
        do {
            try files.moveItem(at: backup, to: destination)
            return AppIdentity.read(from: destination).bundleIdentifier == DictatorIdentity.bundleIdentifier
        } catch {
            return false
        }
    }

    /// Quarantine is an xattr, not part of the code signature. A stapled ticket stays valid.
    static func stripQuarantine(_ url: URL) {
        removexattr(url.path, "com.apple.quarantine", 0)
        guard let enumerator = FileManager.default.enumerator(at: url, includingPropertiesForKeys: nil) else { return }
        for case let item as URL in enumerator {
            removexattr(item.path, "com.apple.quarantine", 0)
        }
    }
}

/// A copy of the staged app on the destination's volume, when the download is elsewhere.
private struct VolumeStaging {
    var ready: URL?
    private var directory: URL?

    func cleanup() {
        guard let directory else { return }
        try? FileManager.default.removeItem(at: directory)
    }

    static func prepare(_ staged: URL, beside destination: URL) -> VolumeStaging {
        if sameVolume(staged, destination) {
            return VolumeStaging(ready: staged, directory: nil)
        }
        guard let directory = try? FileManager.default.url(
            for: .itemReplacementDirectory,
            in: .userDomainMask,
            appropriateFor: destination,
            create: true
        ) else {
            return VolumeStaging(ready: nil, directory: nil)
        }
        let copy = directory.appending(path: "\(DictatorIdentity.executableName).app")
        do {
            try FileManager.default.copyItem(at: staged, to: copy)
        } catch {
            try? FileManager.default.removeItem(at: directory)
            return VolumeStaging(ready: nil, directory: nil)
        }
        guard InstallPolicy.signatureRefusal(SignedApp.trust(at: copy)) == nil else {
            try? FileManager.default.removeItem(at: directory)
            return VolumeStaging(ready: nil, directory: nil)
        }
        return VolumeStaging(ready: copy, directory: directory)
    }

    private static func sameVolume(_ left: URL, _ right: URL) -> Bool {
        let key = URLResourceKey.volumeIdentifierKey
        guard let a = try? left.resourceValues(forKeys: [key]).volumeIdentifier,
              let b = try? right.resourceValues(forKeys: [key]).volumeIdentifier
        else { return false }
        return (a as AnyObject).isEqual(b)
    }
}

@objc public protocol DictatorUpdateInstalling {
    func install(stagedAppPath: String, reply: @escaping (String?) -> Void)
    func removePreviousCopy(reply: @escaping (String?) -> Void)
}
