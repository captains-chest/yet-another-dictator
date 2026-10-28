import Foundation

/// Gets the model onto disk: the status, retry, coalescing and error wording behind the app's
/// first-launch download (ADR 0004). UI-free, so it is unit-tested with a fake transport; the app
/// wraps it in a `@MainActor` observable facade.
///
/// - `ensureInstalled()` returns at once when the model is already verified.
/// - Concurrent calls share one download.
/// - After a failure the next call starts again; `ModelInstaller` resumes from the `.part` files.
public actor ModelProvisioner {
    public enum Status: Sendable, Equatable {
        case checking
        case downloading(ModelInstaller.Progress)
        case failed(String)
        case ready
    }

    public private(set) var status: Status = .checking
    public nonisolated let installer: ModelInstaller
    private let transport: any ModelTransport
    private let onProgress: @Sendable (ModelInstaller.Progress) -> Void
    private var running: Task<Bool, Never>?

    /// `onProgress` is called from the download as bytes arrive, on no particular thread.
    public init(
        installer: ModelInstaller,
        transport: any ModelTransport,
        onProgress: @escaping @Sendable (ModelInstaller.Progress) -> Void = { _ in }
    ) {
        self.installer = installer
        self.transport = transport
        self.onProgress = onProgress
    }

    /// Downloads whatever is missing; true once the model is complete and verified.
    public func ensureInstalled() async -> Bool {
        if let running { return await running.value }
        if installer.isInstalled {
            status = .ready
            return true
        }
        let installer = installer
        let transport = transport
        let onProgress = onProgress
        status = .downloading(.init(completedBytes: 0, totalBytes: installer.totalBytes))
        let task = Task<Bool, Never> {
            do {
                try await installer.install(using: transport) { progress in onProgress(progress) }
                return true
            } catch {
                await self.fail(error)
                return false
            }
        }
        running = task
        let ok = await task.value
        running = nil
        if ok { status = .ready }
        return ok
    }

    private func fail(_ error: any Error) {
        status = .failed(Self.describe(error))
    }

    /// The wording users see after "Model download failed: ".
    public static func describe(_ error: any Error) -> String {
        if let error = error as? URLError {
            switch error.code {
            case .notConnectedToInternet, .networkConnectionLost, .timedOut, .cannotFindHost:
                return "No internet connection. Dictator retries when you choose Retry."
            default: break
            }
        }
        if let error = error as? CocoaError, error.code == .fileWriteOutOfSpace {
            return "Not enough disk space (about 1.7 GB needed)."
        }
        return String(describing: error)
    }
}
