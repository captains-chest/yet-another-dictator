import DictationCore
import Foundation
import Observation
import OSLog
import WhisperTranscription

/// Owns the speech-model lifecycle (ADR 0007): which model is chosen, which one the engine is
/// using, downloading with progress/resume/retry, and freeing disk space. Nothing is downloaded
/// until a model is chosen (ADR 0006). Audio never leaves the Mac.
@MainActor
@Observable
final class ModelLibrary {
    typealias Status = ModelProvisioner.Status
    static let key = "speechModel"

    private static let log = Logger(subsystem: "com.captainduckay.dictator", category: "model")

    /// The model the user chose; nil until they pick one.
    private(set) var selected: SpeechModel?
    /// The model the engine is transcribing with right now.
    private(set) var inUse: SpeechModel?
    /// Status of the selected model only.
    private(set) var status: Status = .checking
    /// Bumped by every change, so changes the views show (installed/deleted) are observed.
    private(set) var revision = 0
    private var downloads: [SpeechModel: Int] = [:]

    /// Attempts per file before a download fails. Tests lower it to skip the back-off.
    @ObservationIgnored var downloadAttempts = 5
    @ObservationIgnored private var provisioners: [SpeechModel: ModelProvisioner] = [:]
    @ObservationIgnored private let manifest: ModelManifest?
    @ObservationIgnored private let root: URL
    @ObservationIgnored private let transport: any ModelTransport
    @ObservationIgnored private let defaults: UserDefaults
    @ObservationIgnored private let engine: ActiveTranscriber
    @ObservationIgnored private let makeTranscriber: @Sendable (SpeechModel, URL) -> any Transcribing
    @ObservationIgnored private let onActivated: @MainActor () async -> Void
    @ObservationIgnored private let manifestError: String?

    init(
        manifest: ModelManifest?,
        root: URL,
        transport: any ModelTransport,
        defaults: UserDefaults,
        engine: ActiveTranscriber,
        makeTranscriber: @escaping @Sendable (SpeechModel, URL) -> any Transcribing,
        onActivated: @escaping @MainActor () async -> Void,
        manifestError: String? = nil
    ) {
        self.manifest = manifest
        self.root = root
        self.transport = transport
        self.defaults = defaults
        self.engine = engine
        self.makeTranscriber = makeTranscriber
        self.onActivated = onActivated
        self.manifestError = manifestError
        if let manifestError { status = .failed(manifestError) }
        selected = defaults.string(forKey: Self.key).flatMap(SpeechModel.init(rawValue:))
        // A copy that downloaded a model before the picker existed keeps using it without asking.
        if selected == nil, let existing = SpeechModel.ranked.first(where: isInstalled) {
            persist(existing)
        }
    }

    /// The app's bundled manifest and its Application Support folder.
    static func bundled(
        transport: any ModelTransport,
        defaults: UserDefaults,
        engine: ActiveTranscriber,
        makeTranscriber: @escaping @Sendable (SpeechModel, URL) -> any Transcribing,
        onActivated: @escaping @MainActor () async -> Void
    ) -> ModelLibrary {
        do {
            return ModelLibrary(
                manifest: try ModelManifest.bundled(), root: try ModelInstaller.defaultRoot(), transport: transport,
                defaults: defaults, engine: engine, makeTranscriber: makeTranscriber, onActivated: onActivated
            )
        } catch {
            return ModelLibrary(
                manifest: nil, root: URL(filePath: "/nonexistent", directoryHint: .isDirectory), transport: transport,
                defaults: defaults, engine: engine, makeTranscriber: makeTranscriber, onActivated: onActivated,
                manifestError: "The app's model manifest is missing or invalid: \(error)"
            )
        }
    }

    /// The first-launch picker is shown only while this is true.
    var needsChoice: Bool { selected == nil }

    func bytes(of model: SpeechModel) -> Int64 { manifest?.totalBytes(for: model.rawValue) ?? 0 }

    func isInstalled(_ model: SpeechModel) -> Bool {
        _ = revision
        return installer(for: model)?.isInstalled ?? false
    }

    func isDownloading(_ model: SpeechModel) -> Bool { (downloads[model] ?? 0) > 0 }

    /// Only a model that is on disk, not chosen, not loaded and not downloading can be deleted.
    func canDelete(_ model: SpeechModel) -> Bool {
        isInstalled(model) && model != selected && model != inUse && !isDownloading(model)
    }

    /// At launch: loads the chosen model (downloading only if missing). With no choice, fetches
    /// nothing and only lets the controller show "choose a model".
    func start() async {
        guard let selected else {
            await onActivated()
            return
        }
        await activate(selected)
    }

    /// Persists the choice, then downloads (if needed) and loads only that model. If the user
    /// chooses again meanwhile, the latest choice wins.
    func choose(_ model: SpeechModel) async {
        if model != selected { status = .checking }
        persist(model)
        await activate(model)
    }

    /// Tries the selected model's download again; it resumes from the partial files.
    func retry() async {
        guard let selected else { return }
        await activate(selected)
    }

    /// Frees the disk space of a model. Does nothing unless `canDelete` is true.
    func delete(_ model: SpeechModel) throws {
        guard canDelete(model) else { return }
        defer { revision += 1 }
        do {
            try installer(for: model)?.uninstall()
        } catch {
            Self.log.error("deleting model failed: \(String(describing: error), privacy: .public)")
            throw error
        }
        Self.log.notice("model deleted: \(model.rawValue, privacy: .public)")
    }

    var statusText: String? {
        if selected == nil, manifestError == nil { return "No speech model chosen" }
        switch status {
        case .checking, .ready: return nil
        case .downloading(let progress):
            return "Downloading speech model… \(Self.megabytes(progress.completedBytes)) of \(Self.megabytes(progress.totalBytes)) MB"
        case .failed(let reason): return "Model download failed: \(reason)"
        }
    }

    static func megabytes(_ bytes: Int64) -> Int64 { bytes / 1_000_000 }

    /// "464 MB" / "1.6 GB".
    static func size(_ bytes: Int64) -> String {
        bytes >= 1_000_000_000
            ? String(format: "%.1f GB", Double(bytes) / 1_000_000_000)
            : "\(megabytes(bytes)) MB"
    }

    // MARK: - Private

    private func persist(_ model: SpeechModel) {
        selected = model
        defaults.set(model.rawValue, forKey: Self.key)
    }

    private func installer(for model: SpeechModel) -> ModelInstaller? {
        guard let manifest else { return nil }
        var installer = ModelInstaller(manifest: manifest, modelID: model.rawValue, root: root)
        installer.maxAttempts = downloadAttempts
        return installer
    }

    /// One provisioner per model for the library's lifetime, so choosing a model again joins its
    /// running download instead of starting a second one on the same `.part` files.
    private func provisioner(for model: SpeechModel) -> ModelProvisioner? {
        if let existing = provisioners[model] { return existing }
        guard let installer = installer(for: model) else { return nil }
        let provisioner = ModelProvisioner(installer: installer, transport: transport) { [weak self] progress in
            Task { @MainActor in self?.progressed(progress, for: model) }
        }
        provisioners[model] = provisioner
        return provisioner
    }

    private func activate(_ model: SpeechModel) async {
        guard let provisioner = provisioner(for: model) else { return }
        let installer = provisioner.installer
        if !installer.isInstalled {
            if selected == model { status = .downloading(.init(completedBytes: 0, totalBytes: installer.totalBytes)) }
            Self.log.notice("model download started: \(model.rawValue, privacy: .public)")
        }
        downloads[model, default: 0] += 1
        let ok = await provisioner.ensureInstalled()
        downloads[model, default: 1] -= 1
        revision += 1
        // The user may have chosen another model while this one downloaded.
        guard selected == model else { return }
        status = await provisioner.status
        guard selected == model else { return }
        guard ok else {
            if case .failed(let reason) = status {
                Self.log.error("model download failed: \(reason, privacy: .public)")
            }
            return
        }
        guard inUse != model else { return }
        await engine.use(makeTranscriber(model, installer.folder))
        inUse = model
        Self.log.notice("model ready: \(model.rawValue, privacy: .public)")
        await onActivated()
    }

    /// Progress can arrive after the outcome, or for a model no longer selected; only a running
    /// download of the selected model shows it.
    private func progressed(_ progress: ModelInstaller.Progress, for model: SpeechModel) {
        guard model == selected, case .downloading = status else { return }
        status = .downloading(progress)
    }
}
