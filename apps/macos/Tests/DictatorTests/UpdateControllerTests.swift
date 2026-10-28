import AppUpdate
import DictationCore
import Foundation
import Testing
@testable import Dictator

final class FakeReleaseFetching: ReleaseFetching, @unchecked Sendable {
    struct Offline: LocalizedError { var errorDescription: String? { "offline" } }

    private let lock = NSLock()
    private var _calls = 0
    private var _result: Result<OfferedUpdate?, Error> = .success(nil)

    var calls: Int { lock.withLock { _calls } }
    func reply(_ result: Result<OfferedUpdate?, Error>) { lock.withLock { _result = result } }

    func newestRelease(above current: AppVersion) async throws -> OfferedUpdate? {
        let result = lock.withLock { _calls += 1; return _result }
        return try result.get()
    }
}

@MainActor
final class FakeUpdatePresenting: UpdatePresenting {
    var presented: [String?] = []
    var flashes: [(message: String, isInfo: Bool)] = []
    func present(_ status: String?) { presented.append(status) }
    func flash(_ message: String, isInfo: Bool) { flashes.append((message, isInfo)) }
}

@MainActor
@Suite struct UpdateControllerTests {
    let fetcher = FakeReleaseFetching()
    let presenter = FakeUpdatePresenting()
    let defaults: UserDefaults
    let controller: UpdateController

    init() {
        let suite = "UpdateControllerTests.\(UUID().uuidString)"
        defaults = UserDefaults(suiteName: suite)!
        defaults.removePersistentDomain(forName: suite)
        controller = UpdateController(
            fetcher: fetcher, presenter: presenter, defaults: defaults,
            currentVersion: { AppVersion("0.4.0") }
        )
    }

    static func offer(_ version: String) -> OfferedUpdate {
        let page = URL(string: "https://example.com/\(version)")!
        return OfferedUpdate(
            version: AppVersion(version)!, pageURL: page,
            zipURL: page.appendingPathComponent("Dictator.zip"),
            checksumURL: page.appendingPathComponent("Dictator.zip.sha256"), zipBytes: 10
        )
    }

    @Test func autoCheckNotifiesOncePerVersion() async {
        fetcher.reply(.success(Self.offer("0.5.0")))
        await controller.check(userInitiated: false)
        await controller.check(userInitiated: false)
        #expect(fetcher.calls == 2)
        #expect(presenter.flashes.count == 1)
        #expect(presenter.flashes.first?.isInfo == true)
        #expect(presenter.flashes.first?.message.contains("0.5.0") == true)
        #expect(defaults.string(forKey: "notifiedUpdateVersion") == "0.5.0")
        #expect(controller.available == Self.offer("0.5.0"))
    }

    @Test func manualCheckSaysUpToDate() async {
        await controller.check(userInitiated: true)
        #expect(presenter.flashes.map(\.message) == ["Dictator 0.4.0 is up to date."])
        #expect(presenter.flashes.first?.isInfo == true)
    }

    @Test func manualFailureFlashesError() async {
        fetcher.reply(.failure(FakeReleaseFetching.Offline()))
        await controller.check(userInitiated: true)
        #expect(presenter.flashes.count == 1)
        #expect(presenter.flashes.first?.isInfo == false)
        #expect(presenter.flashes.first?.message.hasPrefix("Couldn't check for updates") == true)
    }

    @Test func autoFailureIsSilent() async {
        fetcher.reply(.failure(FakeReleaseFetching.Offline()))
        await controller.check(userInitiated: false)
        #expect(fetcher.calls == 1)
        #expect(presenter.flashes.isEmpty)
    }

    @Test func checksOffNeverFetches() async {
        controller.checksForUpdates = false
        #expect(!controller.shouldAutoCheck(now: Date()))
        await controller.checkIfDue(now: Date())
        #expect(fetcher.calls == 0)
        #expect(defaults.object(forKey: "checkForUpdates") as? Bool == false)
    }

    @Test func checksOnWithNoPreviousCheckFetches() async {
        #expect(controller.shouldAutoCheck(now: Date()))
        await controller.checkIfDue(now: Date())
        #expect(fetcher.calls == 1)
    }

    @Test func downloadingBlocksChecks() async {
        controller.apply(.downloading(0))
        #expect(!controller.phase.allowsCheck)
        controller.checkNow()
        await controller.check(userInitiated: true)
        await Task.yield()
        #expect(fetcher.calls == 0)
    }

    @Test func successfulCheckClearsFailure() async {
        controller.fail(.download)
        guard case .failed = controller.phase else { Issue.record("expected failed"); return }
        await controller.check(userInitiated: false)
        #expect(controller.phase == .idle)
    }

    @Test func progressOnlyMovesForward() {
        controller.apply(.downloading(0.1))
        controller.apply(.downloading(0.1))
        controller.apply(.downloading(0.05))
        controller.apply(.downloading(0.5))
        #expect(presenter.presented == ["Downloading update… 10%", "Downloading update… 50%"])
        controller.fail(.download)
        #expect(presenter.presented.last == .some(nil))
    }
}
