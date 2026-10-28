import AppUpdate
import Foundation
import OSLog

private let log = Logger(subsystem: "com.captainduckay.dictator.updater", category: "install")

final class UpdateService: NSObject, DictatorUpdateInstalling {
    func install(stagedAppPath: String, reply: @escaping (String?) -> Void) {
        guard let host = AppBundleLocation.hostApplication(containingXPC: Bundle.main.bundleURL) else {
            reply("Couldn't find the Dictator app to update.")
            return
        }
        let outcome = AppReplacer.install(stagedApp: URL(fileURLWithPath: stagedAppPath), replacing: host)
        if let outcome {
            log.error("\(outcome, privacy: .public)")
        } else {
            log.notice("replaced the app")
        }
        reply(outcome)
    }

    func removePreviousCopy(reply: @escaping (String?) -> Void) {
        guard let host = AppBundleLocation.hostApplication(containingXPC: Bundle.main.bundleURL) else {
            reply(nil)
            return
        }
        let outcome = AppReplacer.removePreviousCopy(beside: host)
        if let outcome { log.error("\(outcome, privacy: .public)") }
        reply(outcome)
    }
}

final class ServiceDelegate: NSObject, NSXPCListenerDelegate {
    func listener(_ listener: NSXPCListener, shouldAcceptNewConnection connection: NSXPCConnection) -> Bool {
        guard let executable = AppBundleLocation.hostExecutable(containingXPC: Bundle.main.bundleURL),
              let requirement = ClientRequirement.designatedRequirement(of: executable)
        else {
            log.error("refusing connections; the app signature is unreadable")
            return false
        }
        connection.exportedInterface = NSXPCInterface(with: DictatorUpdateInstalling.self)
        connection.exportedObject = UpdateService()
        connection.setCodeSigningRequirement(requirement)
        connection.resume()
        return true
    }
}

let delegate = ServiceDelegate()
let listener = NSXPCListener.service()
listener.delegate = delegate
listener.resume()
RunLoop.main.run()
