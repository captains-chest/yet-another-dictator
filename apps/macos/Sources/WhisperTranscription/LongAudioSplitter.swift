/// Splits a recording longer than Whisper's 30 s window into pieces that are each transcribed whole.
///
/// Whisper hears at most 30 s at a time. Given longer audio, WhisperKit (decoding without
/// timestamps, as `ModelSession` (WhisperKit) does) cuts it into back-to-back 30 s windows: a word that
/// crosses a window boundary is split in two, and a last window shorter than 1 s
/// (`DecodingOptions.windowClipTime`) is skipped, so the end of the dictation goes missing.
///
/// Cutting at the quietest moment in the second half of each window keeps words whole, and keeping
/// every piece at least `minLength` long means none is skipped. Audio of up to 30 s is one piece,
/// exactly as before.
public enum LongAudioSplitter {
    /// WhisperKit's window: 30 s at 16 kHz.
    public static let maxLength = 480_000
    /// Shortest piece a split may leave: 2 s, comfortably above WhisperKit's 1 s clip.
    public static let minLength = 32_000
    /// The stretch whose loudness is compared when looking for a pause: 0.2 s.
    static let quietWindow = 3_200
    /// Distance between candidate cut points: 10 ms.
    static let step = 160

    /// Consecutive ranges that cover `samples` exactly, each at most `maxLength` long. When the audio
    /// is split, every piece is at least `minLength` long.
    public static func ranges(
        for samples: [Float],
        maxLength: Int = maxLength,
        minLength: Int = minLength
    ) -> [Range<Int>] {
        precondition(minLength > 0 && minLength * 2 <= maxLength, "minLength must be at most half of maxLength")
        guard samples.count > maxLength else { return [0..<samples.count] }

        let energy = PrefixEnergy(samples)
        var pieces: [Range<Int>] = []
        var start = 0
        while samples.count - start > maxLength {
            // Cut in the second half of the window, and leave at least minLength for what follows.
            let earliest = start + maxLength / 2
            let latest = min(start + maxLength, samples.count - minLength)
            let cut = quietestPoint(in: earliest...latest, energy: energy)
            pieces.append(start..<cut)
            start = cut
        }
        pieces.append(start..<samples.count)
        return pieces
    }

    /// The candidate point whose surrounding `quietWindow` holds the least energy. On a tie the
    /// later point wins, so pieces stay close to the full window.
    private static func quietestPoint(in candidates: ClosedRange<Int>, energy: PrefixEnergy) -> Int {
        var best = candidates.lowerBound
        var bestEnergy = Double.infinity
        var point = candidates.lowerBound
        while true {
            let value = energy.sum(point - quietWindow / 2, point + quietWindow / 2)
            if value <= bestEnergy {
                best = point
                bestEnergy = value
            }
            if point == candidates.upperBound { break }
            point = min(point + step, candidates.upperBound)
        }
        return best
    }

    /// Running sum of squared samples, so any stretch's energy costs two lookups.
    private struct PrefixEnergy {
        private let sums: [Double]

        init(_ samples: [Float]) {
            var sums = [Double](repeating: 0, count: samples.count + 1)
            for (index, sample) in samples.enumerated() {
                sums[index + 1] = sums[index] + Double(sample) * Double(sample)
            }
            self.sums = sums
        }

        /// Energy of `lower..<upper`, clamped to the recording.
        func sum(_ lower: Int, _ upper: Int) -> Double {
            let lower = max(0, min(lower, sums.count - 1))
            let upper = max(lower, min(upper, sums.count - 1))
            return sums[upper] - sums[lower]
        }
    }
}
