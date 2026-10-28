import AppUpdate
import Foundation

/// What the updater said, or that it didn't answer in time.
enum UpdaterReply: Equatable, Sendable {
    /// nil is success; otherwise a short message to show.
    case replied(String?)
    /// The updater can't be cancelled, so it may still be working.
    case timedOut
}

/// Talks to the embedded updater. Connection failures become a short message; a nil reply is success.
enum UpdateClient {
    /// Signature and Gatekeeper checks plus a cross-volume copy can take minutes.
    static let installTimeout = Duration.seconds(5 * 60)
    static let cleanupTimeout = Duration.seconds(60)

    static func install(stagedApp: URL) async -> UpdaterReply {
        await call(timeout: installTimeout) { service, reply in
            service.install(stagedAppPath: stagedApp.path, reply: reply)
        }
    }

    static func removePreviousCopy() async -> String? {
        let reply = await call(timeout: cleanupTimeout) { service, reply in
            service.removePreviousCopy(reply: reply)
        }
        switch reply {
        case .replied(let message): return message
        case .timedOut: return "The updater took too long to remove the previous copy."
        }
    }

    private static func call(
        timeout: Duration,
        _ body: (DictatorUpdateInstalling, @escaping (String?) -> Void) -> Void
    ) async -> UpdaterReply {
        let exchange = Exchange()
        return await withCheckedContinuation { continuation in
            exchange.start(continuation: continuation, timeout: timeout, body: body)
        }
    }
}

private final class Exchange: @unchecked Sendable {
    private let lock = NSLock()
    private var continuation: CheckedContinuation<UpdaterReply, Never>?
    private var connection: NSXPCConnection?
    private var timeout: Task<Void, Never>?
    private var finished = false

    func start(
        continuation: CheckedContinuation<UpdaterReply, Never>,
        timeout: Duration,
        body: (DictatorUpdateInstalling, @escaping (String?) -> Void) -> Void
    ) {
        self.continuation = continuation
        let connection = NSXPCConnection(serviceName: DictatorIdentity.updaterServiceName)
        connection.remoteObjectInterface = NSXPCInterface(with: DictatorUpdateInstalling.self)
        // Hop off the XPC callback. Resuming the continuation synchronously from `resume()`
        // (the service is missing) traps.
        connection.invalidationHandler = { [weak self] in Task { self?.finish(.replied("Couldn't reach the updater.")) } }
        connection.interruptionHandler = { [weak self] in Task { self?.finish(.replied("Couldn't reach the updater.")) } }
        self.connection = connection
        connection.resume()
        self.timeout = Task { [weak self] in
            try? await Task.sleep(for: timeout)
            self?.finish(.timedOut)
        }
        guard let service = connection.remoteObjectProxyWithErrorHandler({ [weak self] _ in
            self?.finish(.replied("Couldn't reach the updater."))
        }) as? DictatorUpdateInstalling else {
            finish(.replied("Couldn't reach the updater."))
            return
        }
        body(service) { [weak self] message in self?.finish(.replied(message)) }
    }

    private func finish(_ reply: UpdaterReply) {
        lock.lock()
        if finished { lock.unlock(); return }
        finished = true
        let continuation = self.continuation
        self.continuation = nil
        let connection = self.connection
        let timeout = self.timeout
        lock.unlock()
        timeout?.cancel()
        connection?.invalidate()
        continuation?.resume(returning: reply)
    }
}
