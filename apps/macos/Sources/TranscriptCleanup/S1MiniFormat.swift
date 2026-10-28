import Foundation

/// The prompt format S1-mini by Superwhisper was trained on, from its model card. Kept exact:
/// the model is small and only behaves as documented with this prompt.
public enum S1MiniFormat {
    /// The name the model must always be shown under (its licence requires this exact wording).
    public static let displayName = "S1-mini by Superwhisper"

    /// Verbatim from the model card.
    public static let systemPrompt = "You are a text normalizer for speech-to-text transcripts. The input begins with a control line specifying the styling, structure, and context settings; clean the transcript to match those settings and output only the cleaned text."

    /// The input should stay under about 1000 tokens; longer transcripts are cleaned in chunks.
    public static let maxInputTokens = 1000

    /// `[Styling: semi-formal] [Structure: prose] [Context: general]`
    public static func controlLine(_ style: CleanupStyle) -> String {
        "[Styling: \(style.styling.rawValue)] [Structure: \(style.structure.rawValue)] [Context: \(style.context.rawValue)]"
    }

    /// The full chat prompt, ending in the assistant turn with an empty think block
    /// (`enable_thinking=False`), so the model answers directly.
    public static func prompt(for transcript: String, style: CleanupStyle) -> String {
        "<|im_start|>system\n\(systemPrompt)<|im_end|>\n"
            + "<|im_start|>user\n\(controlLine(style))\n\(transcript)<|im_end|>\n"
            + "<|im_start|>assistant\n<think>\n\n</think>\n\n"
    }

    /// The model card's ceiling on generated tokens: 1.3 × input tokens + 32.
    public static func maxNewTokens(forInputTokens count: Int) -> Int {
        (max(count, 0) * 13 + 9) / 10 + 32  // integer ceil(1.3 × count), free of rounding error
    }

    /// The answer from raw model output: any stray think block and surrounding whitespace removed.
    public static func answer(from output: String) -> String {
        var text = output
        if let end = text.range(of: "</think>") {
            text = String(text[end.upperBound...])
        }
        text = text.replacing("<|im_end|>", with: "")
        return text.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    /// Splits `transcript` into pieces of at most `maxTokens` tokens, at sentence ends where
    /// possible and at spaces inside a sentence that is too long by itself. Joining the pieces with
    /// single spaces gives back the transcript with its whitespace collapsed.
    public static func chunks(
        _ transcript: String,
        maxTokens: Int = maxInputTokens,
        tokenCount: (String) -> Int
    ) -> [String] {
        let text = transcript.split(whereSeparator: \.isWhitespace).joined(separator: " ")
        guard !text.isEmpty else { return [] }
        guard tokenCount(text) > maxTokens else { return [text] }

        var pieces: [String] = []
        var current = ""
        func flush() {
            if !current.isEmpty { pieces.append(current) }
            current = ""
        }
        func add(_ unit: String) {
            let candidate = current.isEmpty ? unit : current + " " + unit
            if tokenCount(candidate) <= maxTokens {
                current = candidate
            } else {
                flush()
                current = unit
            }
        }
        for sentence in sentences(in: text) {
            if tokenCount(sentence) <= maxTokens {
                add(sentence)
            } else {
                flush()
                for word in sentence.split(separator: " ").map(String.init) { add(word) }
                flush()
            }
        }
        flush()
        return pieces
    }

    /// Sentences, each keeping its closing punctuation. A sentence ends at `.`, `!`, `?` or `…`
    /// followed by a space.
    static func sentences(in text: String) -> [String] {
        var result: [String] = []
        var current = ""
        var previous: Character?
        for character in text {
            if character == " ", let previous, ".!?…".contains(previous) {
                result.append(current)
                current = ""
            } else {
                current.append(character)
            }
            previous = character
        }
        if !current.isEmpty { result.append(current) }
        return result
    }
}
