import Testing
@testable import TranscriptCleanup

struct S1MiniFormatTests {
    @Test func systemPromptIsTheModelCardsVerbatim() {
        #expect(S1MiniFormat.systemPrompt == "You are a text normalizer for speech-to-text transcripts. The input begins with a control line specifying the styling, structure, and context settings; clean the transcript to match those settings and output only the cleaned text.")
    }

    @Test func displayNameIsExact() {
        #expect(S1MiniFormat.displayName == "S1-mini by Superwhisper")
    }

    @Test func defaultControlLine() {
        #expect(S1MiniFormat.controlLine(.default) == "[Styling: semi-formal] [Structure: prose] [Context: general]")
    }

    @Test func controlLineUsesOnlyTrainedValues() {
        #expect(CleanupStyle.Styling.allCases.map(\.rawValue) == ["casual", "semi-casual", "semi-formal", "formal"])
        #expect(CleanupStyle.Structure.allCases.map(\.rawValue) == ["prose", "lists"])
        #expect(CleanupStyle.Context.allCases.map(\.rawValue) == ["general", "email"])
        let style = CleanupStyle(styling: .casual, structure: .lists, context: .email)
        #expect(S1MiniFormat.controlLine(style) == "[Styling: casual] [Structure: lists] [Context: email]")
    }

    @Test func promptIsTheModelCardsLiteralWithAnEmptyThinkBlock() {
        let prompt = S1MiniFormat.prompt(for: "so um send the report by uh friday", style: .default)

        #expect(prompt == """
            <|im_start|>system
            You are a text normalizer for speech-to-text transcripts. The input begins with a control line specifying the styling, structure, and context settings; clean the transcript to match those settings and output only the cleaned text.<|im_end|>
            <|im_start|>user
            [Styling: semi-formal] [Structure: prose] [Context: general]
            so um send the report by uh friday<|im_end|>
            <|im_start|>assistant
            <think>

            </think>


            """)
        #expect(prompt.hasSuffix("<|im_start|>assistant\n<think>\n\n</think>\n\n"))
    }

    @Test(arguments: [(0, 32), (1, 34), (10, 45), (100, 162), (1000, 1332)])
    func maxNewTokensIsOnePointThreeTimesInputPlus32(_ input: Int, _ expected: Int) {
        #expect(S1MiniFormat.maxNewTokens(forInputTokens: input) == expected)
    }

    @Test func answerDropsAStrayThinkBlockAndWhitespace() {
        #expect(S1MiniFormat.answer(from: "<think>\n\n</think>\n\nSend it.") == "Send it.")
        #expect(S1MiniFormat.answer(from: "  Send it.\n") == "Send it.")
        #expect(S1MiniFormat.answer(from: "\n\n") == "")
        #expect(S1MiniFormat.answer(from: "Send it.<|im_end|>") == "Send it.")
    }

    // MARK: Chunking

    /// One token per word keeps the expectations readable.
    private static func words(_ text: String) -> Int { text.split(separator: " ").count }

    @Test func shortTranscriptIsOneChunk() {
        #expect(S1MiniFormat.chunks("so um  send it\nnow", tokenCount: Self.words) == ["so um send it now"])
    }

    @Test func emptyTranscriptHasNoChunks() {
        #expect(S1MiniFormat.chunks("  \n ", tokenCount: Self.words) == [])
    }

    @Test func longTranscriptSplitsAtSentenceEnds() {
        let text = "One two three. Four five six! Seven eight? Nine ten."
        let chunks = S1MiniFormat.chunks(text, maxTokens: 6, tokenCount: Self.words)

        #expect(chunks == ["One two three. Four five six!", "Seven eight? Nine ten."])
        #expect(chunks.joined(separator: " ") == text)
    }

    @Test func overlongSentenceSplitsAtSpaces() {
        let text = "Short one. a b c d e f g h i j. End."
        let chunks = S1MiniFormat.chunks(text, maxTokens: 4, tokenCount: Self.words)

        #expect(chunks == ["Short one.", "a b c d", "e f g h", "i j.", "End."])
        #expect(chunks.allSatisfy { Self.words($0) <= 4 })
        #expect(chunks.joined(separator: " ") == text)
    }

    @Test func defaultChunkLimitIsAboutAThousandTokens() {
        #expect(S1MiniFormat.maxInputTokens == 1000)
        let sentence = "this is a sentence of exactly ten words in it."
        let text = Array(repeating: sentence, count: 250).joined(separator: " ")
        let chunks = S1MiniFormat.chunks(text, tokenCount: Self.words)

        #expect(chunks.count == 3)
        #expect(chunks.allSatisfy { Self.words($0) <= 1000 })
        #expect(chunks.joined(separator: " ") == text)
    }

    @Test func sentenceEndsNeedASpaceAfter() {
        #expect(S1MiniFormat.sentences(in: "It costs 4.50 today. Ok… fine") == ["It costs 4.50 today.", "Ok…", "fine"])
    }
}
