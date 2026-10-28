/// Rewrites a raw speech-to-text transcript into the text the user meant: fillers and false starts
/// removed, corrections applied, numbers and punctuation written properly. Must work fully offline.
///
/// The port behind which a cleanup model runs (ADR 0011). The app talks only to this protocol; the
/// llama.cpp adapter lives in its own target, and tests use a fake.
public protocol TranscriptCleaning: Sendable {
    /// Loads the model so the first dictation doesn't pay for it.
    func prepare() async throws
    /// The cleaned text. An empty result is valid: the transcript held nothing worth typing
    /// (for example only "um"). Throws `CancellationError` if the calling task is cancelled.
    func clean(_ transcript: String, style: CleanupStyle) async throws -> String
}
