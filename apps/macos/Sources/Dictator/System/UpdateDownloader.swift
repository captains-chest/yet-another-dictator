import AppUpdate
import Foundation

enum UpdateDownloadError: Error {
    case failed
    case tooLarge
}

/// Downloads one HTTPS file to disk. Redirects are limited to GitHub's release hosts.
enum UpdateDownloader {
    static let requestTimeout: TimeInterval = 30
    static let resourceTimeout: TimeInterval = 120

    static func download(_ url: URL, to destination: URL, limit: Int, progress: @escaping @Sendable (Double) -> Void) async throws {
        guard UpdateDownloadURL.isAllowed(url) else { throw UpdateDownloadError.failed }
        let task = Transfer(url: url, destination: destination, limit: limit, progress: progress)
        try await task.run()
    }
}

private final class Transfer: NSObject, URLSessionDownloadDelegate, URLSessionTaskDelegate, @unchecked Sendable {
    private let url: URL
    private let destination: URL
    private let limit: Int
    private let progress: @Sendable (Double) -> Void
    private let lock = NSLock()
    private var continuation: CheckedContinuation<Void, Error>?
    private var session: URLSession?
    private var finished = false

    init(url: URL, destination: URL, limit: Int, progress: @escaping @Sendable (Double) -> Void) {
        self.url = url
        self.destination = destination
        self.limit = limit
        self.progress = progress
    }

    func run() async throws {
        let configuration = URLSessionConfiguration.ephemeral
        configuration.timeoutIntervalForRequest = UpdateDownloader.requestTimeout
        configuration.timeoutIntervalForResource = UpdateDownloader.resourceTimeout
        let session = URLSession(configuration: configuration, delegate: self, delegateQueue: nil)
        self.session = session
        try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, Error>) in
            self.continuation = continuation
            var request = URLRequest(url: url, cachePolicy: .reloadIgnoringLocalCacheData, timeoutInterval: UpdateDownloader.requestTimeout)
            request.setValue("Dictator-update", forHTTPHeaderField: "User-Agent")
            session.downloadTask(with: request).resume()
        }
    }

    func urlSession(
        _ session: URLSession,
        task: URLSessionTask,
        willPerformHTTPRedirection response: HTTPURLResponse,
        newRequest request: URLRequest,
        completionHandler: @escaping (URLRequest?) -> Void
    ) {
        guard let redirected = request.url, UpdateDownloadURL.isAllowed(redirected) else {
            completionHandler(nil)
            return
        }
        completionHandler(request)
    }

    func urlSession(
        _ session: URLSession,
        downloadTask: URLSessionDownloadTask,
        didWriteData bytesWritten: Int64,
        totalBytesWritten: Int64,
        totalBytesExpectedToWrite: Int64
    ) {
        if totalBytesWritten > limit || (totalBytesExpectedToWrite > limit) {
            downloadTask.cancel()
            return
        }
        let fraction = totalBytesExpectedToWrite > 0 ? Double(totalBytesWritten) / Double(totalBytesExpectedToWrite) : 0
        progress(fraction)
    }

    func urlSession(_ session: URLSession, downloadTask: URLSessionDownloadTask, didFinishDownloadingTo location: URL) {
        let status = (downloadTask.response as? HTTPURLResponse)?.statusCode ?? 0
        guard status == 200 else {
            finish(.failure(UpdateDownloadError.failed))
            return
        }
        do {
            let bytes = try location.resourceValues(forKeys: [.fileSizeKey]).fileSize ?? 0
            guard bytes <= limit else {
                finish(.failure(UpdateDownloadError.tooLarge))
                return
            }
            try? FileManager.default.removeItem(at: destination)
            try FileManager.default.moveItem(at: location, to: destination)
            finish(.success(()))
        } catch {
            finish(.failure(UpdateDownloadError.failed))
        }
    }

    func urlSession(_ session: URLSession, task: URLSessionTask, didCompleteWithError error: Error?) {
        if error != nil { finish(.failure(UpdateDownloadError.failed)) }
    }

    private func finish(_ result: Result<Void, Error>) {
        lock.lock()
        let continuation = self.continuation
        if finished { lock.unlock(); return }
        finished = true
        self.continuation = nil
        lock.unlock()
        session?.finishTasksAndInvalidate()
        continuation?.resume(with: result)
    }
}
