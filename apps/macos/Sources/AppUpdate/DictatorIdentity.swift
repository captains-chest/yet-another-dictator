import Foundation
import Security

/// Constants that identify this app to the updater. The team identifier is the Developer ID
/// team from ADR 0003; it is not a secret.
public enum DictatorIdentity {
    public static let bundleIdentifier = "com.captainduckay.dictator"
    public static let teamIdentifier = "55GY5DC584"
    public static let executableName = "Dictator"
    public static let updaterServiceName = "com.captainduckay.dictator.updater"
    /// Left beside the app after a successful replace, and removed on the next launch.
    public static let previousCopyName = "Dictator.previous.app"

    public static func archiveName(tag: String) -> String {
        "Dictator-\(tag)-macos-arm64.zip"
    }
}

/// Where the running app and its updater live inside the bundle.
public enum AppBundleLocation {
    /// `Dictator.app` containing this XPC bundle, or nil if the layout is wrong.
    public static func hostApplication(containingXPC xpcBundle: URL) -> URL? {
        let app = xpcBundle
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .standardizedFileURL
        guard app.pathExtension == "app" else { return nil }
        return app
    }

    public static func hostExecutable(containingXPC xpcBundle: URL) -> URL? {
        hostApplication(containingXPC: xpcBundle)?
            .appending(path: "Contents/MacOS/\(DictatorIdentity.executableName)")
    }
}

/// The main executable's designated requirement. Only that signed binary may talk to the updater.
public enum ClientRequirement {
    public static func designatedRequirement(of executable: URL) -> String? {
        var code: SecStaticCode?
        guard SecStaticCodeCreateWithPath(executable as CFURL, [], &code) == errSecSuccess, let code else { return nil }
        var requirement: SecRequirement?
        guard SecCodeCopyDesignatedRequirement(code, [], &requirement) == errSecSuccess, let requirement else { return nil }
        var text: CFString?
        guard SecRequirementCopyString(requirement, [], &text) == errSecSuccess else { return nil }
        return text as String?
    }
}

/// Gatekeeper runs the app from a random read-only path when it was opened from a download
/// without being moved. That copy cannot replace itself.
public enum AppLocation {
    public static func isTranslocated(path: String) -> Bool {
        path.contains("/AppTranslocation/")
    }
}
