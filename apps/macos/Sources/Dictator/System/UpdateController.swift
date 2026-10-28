import AppKit
import AppUpdate
import DictationCore
import OSLog

/// The one network call a check needs: the newest release above the running version.
protocol ReleaseFetching: Sendable {
    func newestRelease(above current: AppVersion) async throws -> OfferedUpdate?
}

extension UpdateChecker: ReleaseFetching {}

/// Where update notices go. Update notices only; dictation state never goes through this.
@MainActor
protocol UpdatePresenting: AnyObject {
    func present(_ status: String?)
    func flash(_ message: String, isInfo: Bool)
}

extension OverlayPanel: UpdatePresenting {}

/// Daily check, and installing the release the user picks. Downloading happens only after Install.
@MainActor
@Observable
final class UpdateController {
    private(set) var available: OfferedUpdate?
    private(set) var isChecking = false
    private(set) var phase: UpdatePhase = .idle

    var checksForUpdates: Bool {
        didSet { defaults.set(checksForUpdates, forKey: Self.checksKey) }
    }

    @ObservationIgnored private let fetcher: any ReleaseFetching
    @ObservationIgnored private let presenter: any UpdatePresenting
    @ObservationIgnored private let currentVersion: () -> AppVersion?
    @ObservationIgnored private let defaults: UserDefaults
    @ObservationIgnored private let installer: UpdateInstaller
    @ObservationIgnored private var lastPercent = -1
    private static let checksKey = "checkForUpdates"
    private static let lastCheckKey = "lastUpdateCheck"
    private static let notifiedKey = "notifiedUpdateVersion"
    private static let log = Logger(subsystem: "com.captainduckay.dictator", category: "update")

    init(
        fetcher: any ReleaseFetching,
        presenter: any UpdatePresenting,
        defaults: UserDefaults = .standard,
        installer: UpdateInstaller = UpdateInstaller(),
        currentVersion: @escaping () -> AppVersion? = { WhatsNewWindow.currentVersion }
    ) {
        self.fetcher = fetcher
        self.presenter = presenter
        self.installer = installer
        self.currentVersion = currentVersion
        self.defaults = defaults
        checksForUpdates = defaults.object(forKey: Self.checksKey) as? Bool ?? true
    }

    func start() {
        Task { await runDaily() }
        Task { await removePreviousCopy() }
    }

    func checkNow() {
        Task { await check(userInitiated: true) }
    }

    func openReleasePage() {
        guard let available else { return }
        NSWorkspace.shared.open(available.pageURL)
    }

    func install() {
        guard let available, available.canInstall, phase.allowsCheck else { return }
        if AppLocation.isTranslocated(path: Bundle.main.bundleURL.resolvingSymlinksInPath().path) {
            fail(.translocated)
            return
        }
        // Set before the task starts so a second Install or a check is refused right away.
        lastPercent = -1
        apply(.downloading(0))
        let update = available
        let report: @Sendable (UpdatePhase) -> Void = { [weak self] phase in
            Task { @MainActor in self?.apply(phase) }
        }
        let installer = installer
        Task {
            do {
                try await Task.detached(priority: .userInitiated) {
                    try await installer.install(update, progress: report)
                }.value
                relaunch()
            } catch let error as UpdateInstallError {
                fail(error)
            } catch {
                fail(.download)
            }
        }
    }

    func apply(_ phase: UpdatePhase) {
        switch phase {
        case .downloading(let fraction):
            let percent = Int((min(1, max(0, fraction)) * 100).rounded())
            guard percent > lastPercent else { return }
            lastPercent = percent
            self.phase = .downloading(Double(percent) / 100)
            presenter.present("Downloading update… \(percent)%")
        case .checking:
            self.phase = phase
            presenter.present("Checking update…")
        case .installing:
            self.phase = phase
            presenter.present("Installing update…")
        case .idle, .failed:
            self.phase = phase
        }
    }

    func fail(_ error: UpdateInstallError) {
        phase = .failed(error.message)
        presenter.present(nil)
        presenter.flash(error.message, isInfo: false)
        Self.log.error("\(error.message, privacy: .public)")
    }

    private func relaunch() {
        let configuration = NSWorkspace.OpenConfiguration()
        configuration.createsNewApplicationInstance = true
        SingleInstance.markRelaunch()
        NSWorkspace.shared.openApplication(at: Bundle.main.bundleURL, configuration: configuration) { _, error in
            Task { @MainActor in
                if error != nil {
                    self.presenter.present("Update installed. Quit Dictator and open it again.")
                    return
                }
                NSApp.terminate(nil)
            }
        }
    }

    private func removePreviousCopy() async {
        if let message = await UpdateClient.removePreviousCopy() {
            Self.log.error("\(message, privacy: .public)")
        }
    }

    private func runDaily() async {
        while !Task.isCancelled {
            await checkIfDue(now: Date())
            try? await Task.sleep(for: .seconds(60 * 60))
        }
    }

    /// Whether the automatic check should run now. Off when the user turned checks off.
    func shouldAutoCheck(now: Date) -> Bool {
        checksForUpdates && UpdateCheck.isDue(lastCheck: defaults.object(forKey: Self.lastCheckKey) as? Date, now: now)
    }

    func checkIfDue(now: Date) async {
        if shouldAutoCheck(now: now) { await check(userInitiated: false) }
    }

    func check(userInitiated: Bool) async {
        guard !isChecking, phase.allowsCheck, let current = currentVersion() else { return }
        isChecking = true
        defer { isChecking = false }
        do {
            let update = try await fetcher.newestRelease(above: current)
            defaults.set(Date(), forKey: Self.lastCheckKey)
            available = update
            if phase.allowsCheck, case .failed = phase { phase = .idle }
            if let update {
                let version = update.version.description
                if userInitiated || defaults.string(forKey: Self.notifiedKey) != version {
                    defaults.set(version, forKey: Self.notifiedKey)
                    let verb = update.canInstall ? "install it" : "get it"
                    presenter.flash("Dictator \(version) is available. Open the menu bar menu to \(verb).", isInfo: true)
                }
            } else if userInitiated {
                presenter.flash("Dictator \(current.description) is up to date.", isInfo: true)
            }
            Self.log.notice("update check: \(update?.version.description ?? "none", privacy: .public)")
        } catch {
            Self.log.error("update check failed: \(error.localizedDescription, privacy: .public)")
            if userInitiated { presenter.flash("Couldn't check for updates: \(error.localizedDescription)", isInfo: false) }
        }
    }
}
