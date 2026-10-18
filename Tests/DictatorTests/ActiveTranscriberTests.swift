import Foundation
import Testing
@testable import Dictator

@Suite struct ActiveTranscriberTests {
    @Test func emptyThrowsNoModel() async {
        let engine = ActiveTranscriber()
        await #expect(throws: ActiveTranscriber.NoModel.self) { try await engine.prepare() }
        await #expect(throws: ActiveTranscriber.NoModel.self) {
            _ = try await engine.transcribe([], language: nil)
        }
    }

    @Test func unavailableReasonIsThrownInsteadOfNoModel() async {
        let engine = ActiveTranscriber()
        engine.unavailable("x")
        do {
            try await engine.prepare()
            Issue.record("expected the load reason")
        } catch {
            #expect("\(error)".contains("x"), "got \(error)")
        }
        do {
            _ = try await engine.transcribe([], language: nil)
            Issue.record("expected the load reason")
        } catch {
            #expect("\(error)".contains("x"), "got \(error)")
        }
    }

    @Test func useServesAtOnceAndClearsReason() async throws {
        let engine = ActiveTranscriber()
        engine.unavailable("x")
        engine.use(FakeTranscriber(model: .parakeetV2))
        try await engine.prepare()
        #expect(try await engine.transcribe([], language: nil) == SpeechModel.parakeetV2.rawValue)
        // While serving, a stale failure reason changes nothing.
        engine.unavailable("y")
        try await engine.prepare()
        #expect(try await engine.transcribe([], language: nil) == SpeechModel.parakeetV2.rawValue)
    }
}
