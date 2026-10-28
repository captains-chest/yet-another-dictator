import AVFoundation
import DictationCore
import Foundation
import Testing
import WhisperTranscription
import ModelSession

/// End-to-end check against the real model. Run `scripts/fetch-model.sh` first; skipped otherwise.
struct WhisperKitTranscriberTests {
    static let modelFolder = URL(filePath: #filePath)
        .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        .appending(path: "Model", directoryHint: .isDirectory)

    static var modelAvailable: Bool {
        FileManager.default.fileExists(atPath: modelFolder.appending(path: "tokenizer.json").path(percentEncoded: false))
    }

    @Test(.enabled(if: modelAvailable), .timeLimit(.minutes(10)))
    func transcribesSynthesisedSpeech() async throws {
        let samples = try Self.speak("The quick brown fox jumps over the lazy dog.")
        let transcriber = ModelSession.whisperKit(modelFolder: Self.modelFolder)

        let text = try await transcriber.transcribe(samples, language: nil)

        let normalised = text.lowercased()
        #expect(normalised.contains("quick brown fox"), "got: \(text)")
        #expect(normalised.contains("lazy dog"), "got: \(text)")
    }

    /// Short single sentences are where Whisper used to drop the closing punctuation.
    @Test(.enabled(if: modelAvailable), .timeLimit(.minutes(10)), arguments: [
        "Please send me the report tomorrow",
        "Can you call me back later",
    ])
    func shortSentenceEndsWithPunctuation(_ sentence: String) async throws {
        let transcriber = ModelSession.whisperKit(modelFolder: Self.modelFolder)

        let text = TranscriptCleaner.clean(try await transcriber.transcribe(try Self.speak(sentence), language: nil))

        #expect(text.last.map { ".?!".contains($0) } == true, "got: \(text)")
    }

    @Test(.enabled(if: modelAvailable), .timeLimit(.minutes(10)))
    func silenceDoesNotEchoThePrompt() async throws {
        let transcriber = ModelSession.whisperKit(modelFolder: Self.modelFolder)

        let text = try await transcriber.transcribe(Array(repeating: 0, count: 32_000), language: nil)

        #expect(!text.contains("hvordan går det"), "got: \(text)")
        #expect(!text.contains("how are you"), "got: \(text)")
    }

    /// Over 30 s, WhisperKit used to cut at fixed 30 s windows and skip a last window under 1 s.
    @Test(.enabled(if: modelAvailable), .timeLimit(.minutes(10)))
    func longDictationKeepsItsEnding() async throws {
        // Varied sentences: repeated ones would trip Whisper's repetition check.
        let note = """
            Yesterday we walked along the harbour and watched the ferries leave for the islands. \
            The weather turned cold in the afternoon, so we stopped at a small cafe for coffee. \
            My sister told me about her new job at the library and the books she wants to order. \
            Later we talked about painting the kitchen and replacing the old wooden fence. \
            On Saturday the neighbours are having a party, and we promised to bring a cake. \
            Next month I would like to visit my grandparents, who live near the forest. \
            The train takes about three hours, which gives me time to read and to plan the trip. \
            Afterwards we will write a short report and share the photos with the whole family. \
            This is the very last sentence.
            """
        let samples = try Self.speak(note)
        try #require(samples.count > LongAudioSplitter.maxLength, "speech is only \(samples.count) samples")
        let transcriber = ModelSession.whisperKit(modelFolder: Self.modelFolder)

        let text = try await transcriber.transcribe(samples, language: "en")

        let normalised = text.lowercased()
        #expect(normalised.contains("ferries"), "got: \(text)")
        #expect(normalised.contains("wooden fence"), "got: \(text)")
        #expect(normalised.contains("very last sentence"), "got: \(text)")
    }

    @Test func refusesToLoadIncompleteModelFolder() async throws {
        let empty = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
        try FileManager.default.createDirectory(at: empty, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: empty) }

        do {
            try await ModelSession.whisperKit(modelFolder: empty).prepare()
            Issue.record("expected missing files")
        } catch {
            #expect(
                "\(error)".contains("tokenizer.json"),
                "factory files should name tokenizer.json, got: \(error)"
            )
        }
    }

    /// Uses macOS `say` to render 16 kHz mono float PCM.
    static func speak(_ text: String) throws -> [Float] {
        let file = FileManager.default.temporaryDirectory.appending(path: "\(UUID().uuidString).wav")
        defer { try? FileManager.default.removeItem(at: file) }

        let say = Process()
        say.executableURL = URL(filePath: "/usr/bin/say")
        say.arguments = ["--file-format=WAVE", "--data-format=LEF32@16000", "-o", file.path(percentEncoded: false), text]
        try say.run()
        say.waitUntilExit()

        let audio = try AVAudioFile(forReading: file, commonFormat: .pcmFormatFloat32, interleaved: false)
        let buffer = try #require(AVAudioPCMBuffer(pcmFormat: audio.processingFormat, frameCapacity: AVAudioFrameCount(audio.length)))
        try audio.read(into: buffer)
        let channel = try #require(buffer.floatChannelData?[0])
        return Array(UnsafeBufferPointer(start: channel, count: Int(buffer.frameLength)))
    }
}
