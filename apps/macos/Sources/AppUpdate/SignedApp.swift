import DictationCore
import Foundation
import Security

/// Bundle identifier and version read from Info.plist. Used for the copy being replaced, which
/// may be an ad-hoc local build and is not put through a full signature check.
public struct AppIdentity: Equatable, Sendable {
    public var bundleIdentifier: String?
    public var version: AppVersion?

    public init(bundleIdentifier: String?, version: AppVersion?) {
        self.bundleIdentifier = bundleIdentifier
        self.version = version
    }

    public static func read(from app: URL) -> AppIdentity {
        let plistURL = app.appending(path: "Contents/Info.plist")
        guard let data = try? Data(contentsOf: plistURL),
              let plist = try? PropertyListSerialization.propertyList(from: data, format: nil) as? [String: Any]
        else { return AppIdentity(bundleIdentifier: nil, version: nil) }
        let identifier = plist["CFBundleIdentifier"] as? String
        let version = (plist["CFBundleShortVersionString"] as? String).flatMap(AppVersion.init)
        return AppIdentity(bundleIdentifier: identifier, version: version)
    }
}

/// Whether a bundle is a Developer ID build of Dictator we are willing to install.
public struct ReleaseTrust: Equatable, Sendable {
    public var identity: AppIdentity
    public var teamIdentifier: String?
    public var signatureValid: Bool
    public var hardenedRuntime: Bool
    public var stapledTicket: Bool

    public init(identity: AppIdentity, teamIdentifier: String?, signatureValid: Bool, hardenedRuntime: Bool, stapledTicket: Bool) {
        self.identity = identity
        self.teamIdentifier = teamIdentifier
        self.signatureValid = signatureValid
        self.hardenedRuntime = hardenedRuntime
        self.stapledTicket = stapledTicket
    }
}

public enum SignedApp {
    /// Developer ID Application, our team, our bundle identifier.
    public static let developerIDRequirement = """
    anchor apple generic and certificate 1[field.1.2.840.113635.100.6.2.6] /* exists */ \
    and certificate leaf[field.1.2.840.113635.100.6.1.13] /* exists */ \
    and certificate leaf[subject.OU] = "\(DictatorIdentity.teamIdentifier)" \
    and identifier "\(DictatorIdentity.bundleIdentifier)"
    """

    static let validationFlags = SecCSFlags(rawValue:
        kSecCSCheckAllArchitectures
            | kSecCSCheckNestedCode
            | kSecCSStrictValidate
            | kSecCSRestrictSymlinks
            | kSecCSRestrictToAppLike
    )

    public static func requirementParses() -> Bool {
        var requirement: SecRequirement?
        return SecRequirementCreateWithString(developerIDRequirement as CFString, [], &requirement) == errSecSuccess
    }

    public static func trust(at app: URL) -> ReleaseTrust {
        let identity = AppIdentity.read(from: app)
        guard let code = staticCode(at: app),
              let requirement = parsedRequirement(),
              SecStaticCodeCheckValidityWithErrors(code, validationFlags, requirement, nil) == errSecSuccess
        else {
            return ReleaseTrust(identity: identity, teamIdentifier: nil, signatureValid: false, hardenedRuntime: false, stapledTicket: false)
        }
        var raw: CFDictionary?
        guard SecCodeCopySigningInformation(code, SecCSFlags(rawValue: kSecCSSigningInformation | kSecCSContentInformation), &raw) == errSecSuccess,
              let info = raw as? [String: Any]
        else {
            return ReleaseTrust(identity: identity, teamIdentifier: nil, signatureValid: false, hardenedRuntime: false, stapledTicket: false)
        }
        let flags = SecCodeSignatureFlags(rawValue: flagValue(info[kSecCodeInfoFlags as String]))
        let ticket = info[kSecCodeInfoStapledNotarizationTicket as String] as? Data
        return ReleaseTrust(
            identity: identity,
            teamIdentifier: info[kSecCodeInfoTeamIdentifier as String] as? String,
            signatureValid: true,
            hardenedRuntime: flags.contains(.runtime),
            stapledTicket: ticket?.isEmpty == false
        )
    }

    private static func staticCode(at url: URL) -> SecStaticCode? {
        var code: SecStaticCode?
        guard SecStaticCodeCreateWithPath(url as CFURL, [], &code) == errSecSuccess else { return nil }
        return code
    }

    private static func parsedRequirement() -> SecRequirement? {
        var requirement: SecRequirement?
        guard SecRequirementCreateWithString(developerIDRequirement as CFString, [], &requirement) == errSecSuccess else { return nil }
        return requirement
    }

    private static func flagValue(_ value: Any?) -> UInt32 {
        switch value {
        case let number as NSNumber: number.uint32Value
        case let number as UInt32: number
        case let number as Int: UInt32(clamping: number)
        default: 0
        }
    }
}

public enum InstallPolicy {
    /// Nil when the staged app may replace the destination.
    ///
    /// The destination only has to be Dictator at an older version, so an ad-hoc local build can
    /// move onto a signed release. The staged app must be our notarized Developer ID build.
    /// Gatekeeper's own assessment is the last check, and it runs outside the sandbox.
    public static func refusal(replacing destination: AppIdentity, with staged: ReleaseTrust, gatekeeperAccepted: Bool) -> String? {
        if let problem = signatureRefusal(staged) { return problem }
        if !gatekeeperAccepted { return Self.untrusted }
        return versionRefusal(replacing: destination, with: staged.identity)
    }

    public static func signatureRefusal(_ staged: ReleaseTrust) -> String? {
        guard staged.signatureValid,
              staged.hardenedRuntime,
              staged.stapledTicket,
              staged.teamIdentifier == DictatorIdentity.teamIdentifier,
              staged.identity.bundleIdentifier == DictatorIdentity.bundleIdentifier
        else { return untrusted }
        return nil
    }

    public static func versionRefusal(replacing destination: AppIdentity, with staged: AppIdentity) -> String? {
        guard destination.bundleIdentifier == DictatorIdentity.bundleIdentifier else {
            return "Dictator can only replace its own copy."
        }
        guard let stagedVersion = staged.version, let destinationVersion = destination.version else {
            return "The update's version couldn't be read."
        }
        guard destinationVersion < stagedVersion else {
            return "Dictator \(destinationVersion) is already current."
        }
        return nil
    }

    public static let untrusted = "The download isn't a signed, notarized Dictator release."
}

public enum Gatekeeper {
    /// `spctl` is Gatekeeper's own check, including the stapled ticket. It has to run unsandboxed.
    public static func accepts(_ app: URL) -> Bool {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/usr/sbin/spctl")
        process.arguments = ["--assess", "--type", "execute", "--ignore-cache", "--", app.path]
        process.standardOutput = FileHandle.nullDevice
        process.standardError = FileHandle.nullDevice
        guard (try? process.run()) != nil else { return false }
        process.waitUntilExit()
        return process.terminationStatus == 0
    }
}
