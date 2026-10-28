import Foundation
import Testing
@testable import WhisperTranscription

struct LongAudioSplitterTests {
    static let rate = 16_000

    /// A steady tone, loud enough to count as speech everywhere.
    static func tone(seconds: Double) -> [Float] {
        (0..<Int(seconds * Double(rate))).map { 0.3 * Float(sin(Double($0) * 0.2)) }
    }

    static func expectCovers(_ pieces: [Range<Int>], count: Int, sourceLocation: SourceLocation = #_sourceLocation) {
        #expect(pieces.first?.lowerBound == 0, sourceLocation: sourceLocation)
        #expect(pieces.last?.upperBound == count, sourceLocation: sourceLocation)
        for (previous, next) in zip(pieces, pieces.dropFirst()) {
            #expect(previous.upperBound == next.lowerBound, sourceLocation: sourceLocation)
        }
        for piece in pieces {
            #expect(piece.count <= LongAudioSplitter.maxLength, sourceLocation: sourceLocation)
        }
        if pieces.count > 1 {
            for piece in pieces {
                #expect(piece.count >= LongAudioSplitter.minLength, sourceLocation: sourceLocation)
            }
        }
    }

    @Test(arguments: [0, 1, 16_000, 479_999, 480_000])
    func upToThirtySecondsIsOnePiece(_ count: Int) {
        let samples = [Float](repeating: 0.1, count: count)
        #expect(LongAudioSplitter.ranges(for: samples) == [0..<count])
    }

    @Test func cutsInThePause() {
        // 40 s of "speech" with a 0.3 s pause at 25 s.
        var samples = Self.tone(seconds: 40)
        let pause = 25 * Self.rate..<(25 * Self.rate + 4_800)
        for index in pause { samples[index] = 0 }

        let pieces = LongAudioSplitter.ranges(for: samples)

        #expect(pieces.count == 2)
        let cut = pieces[0].upperBound
        #expect(pause.contains(cut), "cut at \(cut)")
        Self.expectCovers(pieces, count: samples.count)
    }

    /// Just over 30 s used to lose the last half second: WhisperKit skips a window under 1 s.
    @Test func justOverThirtySecondsKeepsALongEnoughTail() {
        let samples = Self.tone(seconds: 30.5)

        let pieces = LongAudioSplitter.ranges(for: samples)

        #expect(pieces.count == 2)
        #expect(pieces[1].count >= LongAudioSplitter.minLength)
        Self.expectCovers(pieces, count: samples.count)
    }

    @Test func withoutAPauseCutsAsLateAsAllowed() {
        // Equally loud everywhere (±0.5 squares exactly, so every stretch has the same energy).
        let samples = (0..<(70 * Self.rate)).map { $0.isMultiple(of: 2) ? Float(0.5) : -0.5 }

        let pieces = LongAudioSplitter.ranges(for: samples)

        #expect(pieces.map(\.count) == [480_000, 480_000, 160_000])
        Self.expectCovers(pieces, count: samples.count)
    }

    @Test(arguments: [30.01, 31, 45.5, 60, 61.9, 90, 600])
    func piecesCoverTheRecording(_ seconds: Double) {
        var samples = Self.tone(seconds: seconds)
        // Some pauses at irregular places.
        for start in stride(from: 7 * Self.rate, to: samples.count - 3_200, by: 11 * Self.rate + 1_234) {
            for index in start..<(start + 3_200) { samples[index] = 0.001 }
        }

        Self.expectCovers(LongAudioSplitter.ranges(for: samples), count: samples.count)
    }
}
