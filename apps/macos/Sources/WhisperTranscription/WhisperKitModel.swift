import DictationCore
import Foundation
import ModelSession
@preconcurrency import WhisperKit

/// On-device transcription with WhisperKit Large v3 Turbo, loaded from the folder `ModelInstaller`
/// downloaded and verified (ADR 0004). Audio never leaves the device.
///
/// WhisperKit itself never touches the network: `download` is disabled and the tokenizer is read
/// from the model folder. Only `ModelInstaller` downloads, against the manifest bundled in the app.
extension ModelSession {
    static let whisperRequiredFiles = [
        "MelSpectrogram.mlmodelc",
        "AudioEncoder.mlmodelc",
        "TextDecoder.mlmodelc",
        "config.json",
        "generation_config.json",
        "tokenizer.json",
        "tokenizer_config.json",
    ]

    /// The WhisperKit model as a session. `ModelSession` (ADR 0008) owns loading and ordering.
    public static func whisperKit(modelFolder: URL) -> ModelSession {
        ModelSession(modelFolder: modelFolder, requiredFiles: whisperRequiredFiles) { folder in
            let config = WhisperKitConfig(
                modelFolder: folder.path(percentEncoded: false),
                tokenizerFolder: folder,
                verbose: false, logLevel: .error, prewarm: true, load: true, download: false
            )
            let engine = LoadedWhisperKit.Engine(try await WhisperKit(config))
            // Pay the first Core ML prediction's one-off setup during "Loading model…".
            _ = try? await engine.whisper.transcribe(
                audioArray: [Float](repeating: 0, count: 16_000),
                decodeOptions: DecodingOptions(
                    verbose: false, task: .transcribe, language: "en",
                    temperatureFallbackCount: 0, sampleLength: 4, detectLanguage: false, withoutTimestamps: true
                )
            )
            return LoadedWhisperKit(engine)
        }
    }
}

/// A loaded WhisperKit model.
final class LoadedWhisperKit: LoadedSpeechModel, @unchecked Sendable {
    /// WhisperKit isn't Sendable. Transcriptions are serialised by `ModelSession`, so it is
    /// never used by two at once.
    final class Engine: @unchecked Sendable {
        let whisper: WhisperKit
        /// `DictationPrompt.text` tokenised once at load; nil if the tokenizer is unavailable.
        let promptTokens: [Int]?

        init(_ whisper: WhisperKit) {
            self.whisper = whisper
            promptTokens = whisper.tokenizer.map { tokenizer in
                // Leading space: the prompt reads as preceding text, as in Whisper's own prompting.
                tokenizer.encode(text: " " + DictationPrompt.text)
                    .filter { $0 < tokenizer.specialTokens.specialTokenBegin }
            }
        }
    }

    /// Output that compresses better than this (as WhisperKit measures it) is too repetitive to be
    /// real speech. Same value as WhisperKit's and OpenAI's default.
    static let compressionRatioThreshold: Float = 2.4

    private let engine: Engine

    init(_ engine: Engine) { self.engine = engine }

    func transcribe(_ samples: [Float], language: String?, stop: StopSignal) async throws -> String {
        try await Self.run(engine, samples, language: language, stop: stop)
    }

    private static func run(
        _ engine: Engine, _ samples: [Float], language: String?, stop: StopSignal
    ) async throws -> String {
        // Audio over 30 s is transcribed piece by piece, cut at pauses, so no word is split at a
        // window boundary and the end isn't skipped. See `LongAudioSplitter`.
        let pieces = LongAudioSplitter.ranges(for: samples)
        var language = language
        var texts: [String] = []
        for piece in pieces {
            try Task.checkCancellation()
            if stop.isSet { throw CancellationError() }
            let audio = Array(samples[piece])
            // The recording as a whole was checked for speech; a silent piece of a long one would
            // only give Whisper room to invent text.
            if pieces.count > 1, !SpeechActivity.containsSpeech(audio) { continue }
            let (text, detected) = try await transcribePiece(engine, audio, language: language, stop: stop)
            // One dictation is one language: later pieces use the language detected in the first
            // piece that held speech.
            if language == nil, !text.isEmpty { language = detected }
            // On silence Whisper may repeat the prompt instead of returning nothing.
            if !DictationPrompt.isEcho(text) { texts.append(text) }
        }
        return texts.joined(separator: " ")
    }

    /// Transcribes audio of at most one 30 s window. Returns the text and the detected language.
    private static func transcribePiece(
        _ engine: Engine, _ samples: [Float], language: String?, stop: StopSignal
    ) async throws -> (text: String, language: String?) {
        let options = DecodingOptions(
            verbose: false,
            task: .transcribe,
            language: language,
            temperature: 0,
            usePrefillPrompt: true,
            // Detect only when no language was chosen.
            detectLanguage: language == nil,
            skipSpecialTokens: true,
            withoutTimestamps: true,
            // A punctuated prompt makes Whisper write complete sentences, including the final
            // full stop it otherwise often drops on short dictations. See `DictationPrompt`.
            promptTokens: engine.promptTokens,
            // Whisper's standard quality checks, spelled out so they're deliberate. A window that
            // fails one is decoded again at a higher temperature. noSpeechThreshold has no effect
            // in WhisperKit 1.1.1 (no-speech probability is not computed); silence is instead
            // rejected before transcription by `SpeechActivity`.
            compressionRatioThreshold: Self.compressionRatioThreshold,
            logProbThreshold: -1.0,
            firstTokenLogProbThreshold: -1.5,
            noSpeechThreshold: 0.6
        )
        let results = try await engine.whisper.transcribe(
            audioArray: samples,
            decodeOptions: options,
            // Returning false ends decoding early; nil means carry on.
            callback: { _ in stop.isSet ? false : nil }
        )
        try Task.checkCancellation()
        // A segment still this repetitive after every fallback is a hallucination loop
        // ("tak tak tak …"), not dictation.
        let text = results
            .flatMap(\.segments)
            .filter { $0.compressionRatio <= Self.compressionRatioThreshold }
            .map(\.text)
            .joined(separator: " ")
        return (text, language == nil ? results.first?.language : language)
    }
}
