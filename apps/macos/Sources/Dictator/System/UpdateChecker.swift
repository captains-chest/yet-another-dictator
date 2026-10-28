import AppUpdate
import DictationCore
import Foundation

/// Asks GitHub's public releases API whether a newer Dictator exists.
///
/// The request carries no token, cookies or user data (an ephemeral session with a fixed
/// User-Agent), so GitHub sees only the IP address and that a Dictator asked. The archive itself
/// is downloaded later, and only after the user chooses Install Update.
struct UpdateChecker: Sendable {
    static let releasesURL = URL(string: "https://api.github.com/repos/captains-chest/yet-another-dictator/releases?per_page=30")!

    enum Failure: LocalizedError {
        case badResponse(Int)
        var errorDescription: String? {
            switch self {
            case .badResponse(let status): "GitHub answered with status \(status)."
            }
        }
    }

    func newestRelease(above current: AppVersion) async throws -> OfferedUpdate? {
        var request = URLRequest(url: Self.releasesURL, cachePolicy: .reloadIgnoringLocalCacheData, timeoutInterval: 20)
        request.setValue("application/vnd.github+json", forHTTPHeaderField: "Accept")
        request.setValue("2022-11-28", forHTTPHeaderField: "X-GitHub-Api-Version")
        request.setValue("Dictator-update-check", forHTTPHeaderField: "User-Agent")
        let session = URLSession(configuration: .ephemeral)
        defer { session.finishTasksAndInvalidate() }
        let (data, response) = try await session.data(for: request)
        let status = (response as? HTTPURLResponse)?.statusCode ?? 0
        guard status == 200 else { throw Failure.badResponse(status) }
        let releases = try JSONDecoder().decode([GitHubRelease].self, from: data)
        return OfferedUpdate.newest(in: releases, current: current)
    }
}
