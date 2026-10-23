import DictationCore
import Foundation
import Observation
import OSLog
import TranscriptCleanup
import WhisperTranscription

/// Optional transcript cleanup with S1-mini by Superwhisper (ADR 0011). Off by default. Nothing is
/// downloaded until the user turns it on; then the pinned model is downloaded and verified with
/// the same manifest and installer as the speech models, loaded, and handed to the
/// `CleaningTranscriber`. Until it is ready, and whenever it fails, dictation types raw text.
@MainActor
@Observable
final class CleanupSettings {
    enum Status: Equatable {
        case off
        case downloading(ModelInstaller.Progress)
        case loading
        case ready
        case failed(String)
    }

    static let modelID = "s1-mini"
    static let enabledKey = "transcriptCleanup"
    static let stylingKey = "transcriptCleanupStyling"
    /// Always shown under exactly this name (the model's licence requires it).
    static let displayName = S1MiniFormat.displayName

    private static let log = Logger(subsystem: "com.captainduckay.dictator", category: "cleanup")

    private(set) var isEnabled: Bool
    private(set) var status: Status = .off
    /// Bumped when the model is installed or deleted, so views showing that are refreshed.
    private(set) var revision = 0

    /// How formal the cleaned text is. Structure and context stay prose / general: dictation is
    /// typed as one line, so lists and email layout would be flattened anyway.
    var styling: CleanupStyle.Styling {
        didSet {
            defaults.set(styling.rawValue, forKey: Self.stylingKey)
            transcriber.style = CleanupStyle(styling: styling)
        }
    }

    /// Attempts per file before a download fails. Tests lower it to skip the back-off.
    @ObservationIgnored var downloadAttempts = 5
    @ObservationIgnored private let manifest: ModelManifest?
    @ObservationIgnored private let manifestError: String?
    @ObservationIgnored private let root: URL
    @ObservationIgnored private let transport: any ModelTransport
    @ObservationIgnored private let defaults: UserDefaults
    @ObservationIgnored private let transcriber: CleaningTranscriber
    @ObservationIgnored private let makeCleaner: @Sendable (URL) -> any TranscriptCleaning
    @ObservationIgnored private var download: Task<Void, any Error>?
    /// Bumped by every on/off change, so a download or load that finishes late is dropped.
    @ObservationIgnored private var generation = 0

    init(
        manifest: ModelManifest?,
        root: URL,
        transport: any ModelTransport,
        defaults: UserDefaults,
        transcriber: CleaningTranscriber,
        makeCleaner: @escaping @Sendable (URL) -> any TranscriptCleaning,
        manifestError: String? = nil
    ) {
        self.manifest = manifest
        self.manifestError = manifestError
        self.root = root
        self.transport = transport
        self.defaults = defaults
        self.transcriber = transcriber
        self.makeCleaner = makeCleaner
        isEnabled = defaults.bool(forKey: Self.enabledKey)
        styling = defaults.string(forKey: Self.stylingKey).flatMap(CleanupStyle.Styling.init(rawValue:)) ?? .semiFormal
        transcriber.style = CleanupStyle(styling: styling)
    }

    /// The app's bundled manifest and its Application Support folder.
    static func bundled(
        transport: any ModelTransport,
        defaults: UserDefaults,
        transcriber: CleaningTranscriber,
        makeCleaner: @escaping @Sendable (URL) -> any TranscriptCleaning
    ) -> CleanupSettings {
        do {
            return CleanupSettings(
                manifest: try ModelManifest.bundled(), root: try ModelInstaller.defaultRoot(), transport: transport,
                defaults: defaults, transcriber: transcriber, makeCleaner: makeCleaner
            )
        } catch {
            return CleanupSettings(
                manifest: nil, root: URL(filePath: "/nonexistent", directoryHint: .isDirectory), transport: transport,
                defaults: defaults, transcriber: transcriber, makeCleaner: makeCleaner,
                manifestError: "The app's model manifest is missing or invalid: \(error)"
            )
        }
    }

    /// Download size of the cleanup model.
    var bytes: Int64 { manifest?.totalBytes(for: Self.modelID) ?? 0 }

    var isInstalled: Bool {
        _ = revision
        return installer?.isInstalled ?? false
    }

    /// The downloaded model can be deleted once cleanup is off.
    var canDelete: Bool { !isEnabled && download == nil && isInstalled }

    /// At launch: loads the model if cleanup is on. Fetches nothing while it is off.
    func start() async {
        guard isEnabled else { return }
        await activate()
    }

    /// Turning cleanup on downloads (if needed) and loads the model; turning it off stops any
    /// download and unloads the model. Dictation types raw text in the meantime.
    func setEnabled(_ enabled: Bool) async {
        guard enabled != isEnabled else { return }
        isEnabled = enabled
        defaults.set(enabled, forKey: Self.enabledKey)
        Self.log.notice("cleanup \(enabled ? "on" : "off", privacy: .public)")
        if enabled {
            await activate()
        } else {
            generation += 1
            download?.cancel()
            download = nil
            transcriber.setCleaner(nil)
            status = .off
        }
    }

    /// Tries the download or load again; the download resumes from the partial files.
    func retry() async {
        guard isEnabled, case .failed = status else { return }
        await activate()
    }

    /// Frees the disk space. Does nothing unless `canDelete` is true.
    func delete() throws {
        guard canDelete else { return }
        defer { revision += 1 }
        try installer?.uninstall()
        Self.log.notice("cleanup model deleted")
    }

    var statusText: String? {
        switch status {
        case .off: nil
        case .downloading(let progress):
            "Downloading \(Self.displayName)… \(ModelLibrary.megabytes(progress.completedBytes)) of \(ModelLibrary.megabytes(progress.totalBytes)) MB. Dictation types raw text until it's ready."
        case .loading: "Loading \(Self.displayName)…"
        case .ready: "Ready. Dictations are cleaned up on this Mac."
        case .failed(let reason): "Cleanup is not available: \(reason) Dictation types raw text."
        }
    }

    // MARK: - Private

    private var installer: ModelInstaller? {
        guard let manifest else { return nil }
        var installer = ModelInstaller(manifest: manifest, modelID: Self.modelID, root: root)
        installer.maxAttempts = downloadAttempts
        return installer
    }

    private func activate() async {
        generation += 1
        let id = generation
        guard let installer else {
            status = .failed(manifestError ?? "The cleanup model is not in the app's manifest.")
            return
        }
        if !installer.isInstalled {
            status = .downloading(.init(completedBytes: 0, totalBytes: installer.totalBytes))
            Self.log.notice("cleanup model download started")
            let transport = transport
            let report: @Sendable (ModelInstaller.Progress) -> Void = { [weak self] progress in
                Task { @MainActor in self?.progressed(progress, id: id) }
            }
            let task = Task { try await installer.install(using: transport, progress: report) }
            download = task
            let result = await task.result
            guard id == generation else { return }
            download = nil
            revision += 1
            if case .failure(let error) = result {
                let reason = ModelProvisioner.describe(error)
                Self.log.error("cleanup model download failed: \(reason, privacy: .public)")
                status = .failed(reason.hasSuffix(".") ? reason : reason + ".")
                return
            }
        }
        status = .loading
        let cleaner = makeCleaner(installer.folder)
        do {
            try await cleaner.prepare()
        } catch {
            guard id == generation else { return }
            Self.log.error("cleanup model failed to load: \(String(describing: error), privacy: .public)")
            status = .failed("The model could not be loaded. Try again, or turn cleanup off and delete it.")
            return
        }
        guard id == generation else { return }
        transcriber.setCleaner(cleaner)
        status = .ready
        Self.log.notice("cleanup model ready")
    }

    private func progressed(_ progress: ModelInstaller.Progress, id: Int) {
        guard id == generation, case .downloading = status else { return }
        status = .downloading(progress)
    }
}
