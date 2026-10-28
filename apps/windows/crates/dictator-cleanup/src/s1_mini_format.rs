//! The prompt format S1-mini by Superwhisper was trained on, from its model card. Kept exact: the
//! model is small and only behaves as documented with this prompt.

use dictator_core::CleanupStyle;

/// The name the model must always be shown under (its licence requires this exact wording).
pub const DISPLAY_NAME: &str = "S1-mini by Superwhisper";

/// Verbatim from the model card.
pub const SYSTEM_PROMPT: &str = "You are a text normalizer for speech-to-text transcripts. The input begins with a control line specifying the styling, structure, and context settings; clean the transcript to match those settings and output only the cleaned text.";

/// The input should stay under about 1000 tokens; longer transcripts are cleaned in chunks.
pub const MAX_INPUT_TOKENS: usize = 1000;

/// `[Styling: semi-formal] [Structure: prose] [Context: general]`
pub fn control_line(style: CleanupStyle) -> String {
    format!(
        "[Styling: {}] [Structure: {}] [Context: {}]",
        style.styling.name(),
        style.structure.name(),
        style.context.name()
    )
}

/// The full chat prompt, ending in the assistant turn with an empty think block
/// (`enable_thinking=False`), so the model answers directly.
pub fn prompt(transcript: &str, style: CleanupStyle) -> String {
    format!(
        "<|im_start|>system\n{SYSTEM_PROMPT}<|im_end|>\n<|im_start|>user\n{}\n{transcript}<|im_end|>\n<|im_start|>assistant\n<think>\n\n</think>\n\n",
        control_line(style)
    )
}

/// The model card's ceiling on generated tokens: 1.3 × input tokens + 32.
pub fn max_new_tokens(input_tokens: usize) -> usize {
    (input_tokens * 13).div_ceil(10) + 32 // integer ceil(1.3 × count), free of rounding error
}

/// The answer from raw model output: any stray think block and surrounding whitespace removed.
pub fn answer(output: &str) -> String {
    let after_think = output
        .find("</think>")
        .map_or(output, |end| &output[end + "</think>".len()..]);
    after_think.replace("<|im_end|>", "").trim().to_owned()
}

/// Splits `transcript` into pieces of at most `max_tokens` tokens, at sentence ends where possible
/// and at spaces inside a sentence that is too long by itself. Joining the pieces with single
/// spaces gives back the transcript with its whitespace collapsed.
pub fn chunks(
    transcript: &str,
    max_tokens: usize,
    token_count: impl Fn(&str) -> usize,
) -> Vec<String> {
    let text = transcript.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.is_empty() {
        return Vec::new();
    }
    if token_count(&text) <= max_tokens {
        return vec![text];
    }

    let mut pieces = Vec::new();
    let mut current = String::new();
    let flush = |pieces: &mut Vec<String>, current: &mut String| {
        if !current.is_empty() {
            pieces.push(std::mem::take(current));
        }
    };
    let add = |pieces: &mut Vec<String>, current: &mut String, unit: &str| {
        let candidate = if current.is_empty() {
            unit.to_owned()
        } else {
            format!("{current} {unit}")
        };
        if token_count(&candidate) <= max_tokens {
            *current = candidate;
        } else {
            flush(pieces, current);
            *current = unit.to_owned();
        }
    };
    for sentence in sentences(&text) {
        if token_count(&sentence) <= max_tokens {
            add(&mut pieces, &mut current, &sentence);
        } else {
            flush(&mut pieces, &mut current);
            for word in sentence.split(' ') {
                add(&mut pieces, &mut current, word);
            }
            flush(&mut pieces, &mut current);
        }
    }
    flush(&mut pieces, &mut current);
    pieces
}

/// Sentences, each keeping its closing punctuation. A sentence ends at `.`, `!`, `?` or `…`
/// followed by a space.
pub fn sentences(text: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut current = String::new();
    let mut previous: Option<char> = None;
    for character in text.chars() {
        if character == ' ' && previous.is_some_and(|p| ".!?…".contains(p)) {
            result.push(std::mem::take(&mut current));
        } else {
            current.push(character);
        }
        previous = Some(character);
    }
    if !current.is_empty() {
        result.push(current);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use dictator_core::{Context, Structure, Styling};

    #[test]
    fn system_prompt_is_the_model_cards_verbatim() {
        assert_eq!(
            SYSTEM_PROMPT,
            "You are a text normalizer for speech-to-text transcripts. The input begins with a control line specifying the styling, structure, and context settings; clean the transcript to match those settings and output only the cleaned text."
        );
    }

    #[test]
    fn display_name_is_exact() {
        assert_eq!(DISPLAY_NAME, "S1-mini by Superwhisper");
    }

    #[test]
    fn default_control_line() {
        assert_eq!(
            control_line(CleanupStyle::default()),
            "[Styling: semi-formal] [Structure: prose] [Context: general]"
        );
    }

    #[test]
    fn control_line_uses_only_trained_values() {
        assert_eq!(
            Styling::ALL.map(Styling::name),
            ["casual", "semi-casual", "semi-formal", "formal"]
        );
        assert_eq!(Structure::ALL.map(Structure::name), ["prose", "lists"]);
        assert_eq!(Context::ALL.map(Context::name), ["general", "email"]);
        let style = CleanupStyle {
            styling: Styling::Casual,
            structure: Structure::Lists,
            context: Context::Email,
        };
        assert_eq!(
            control_line(style),
            "[Styling: casual] [Structure: lists] [Context: email]"
        );
    }

    #[test]
    fn prompt_is_the_model_cards_literal_with_an_empty_think_block() {
        let prompt = prompt(
            "so um send the report by uh friday",
            CleanupStyle::default(),
        );

        assert_eq!(
            prompt,
            "<|im_start|>system
You are a text normalizer for speech-to-text transcripts. The input begins with a control line specifying the styling, structure, and context settings; clean the transcript to match those settings and output only the cleaned text.<|im_end|>
<|im_start|>user
[Styling: semi-formal] [Structure: prose] [Context: general]
so um send the report by uh friday<|im_end|>
<|im_start|>assistant
<think>

</think>

"
        );
        assert!(prompt.ends_with("<|im_start|>assistant\n<think>\n\n</think>\n\n"));
    }

    #[test]
    fn max_new_tokens_is_one_point_three_times_input_plus_32() {
        for (input, expected) in [(0, 32), (1, 34), (10, 45), (100, 162), (1000, 1332)] {
            assert_eq!(max_new_tokens(input), expected, "input {input}");
        }
    }

    #[test]
    fn answer_drops_a_stray_think_block_and_whitespace() {
        assert_eq!(answer("<think>\n\n</think>\n\nSend it."), "Send it.");
        assert_eq!(answer("  Send it.\n"), "Send it.");
        assert_eq!(answer("\n\n"), "");
        assert_eq!(answer("Send it.<|im_end|>"), "Send it.");
    }

    // Chunking

    /// One token per word keeps the expectations readable.
    fn words(text: &str) -> usize {
        text.split(' ').filter(|word| !word.is_empty()).count()
    }

    #[test]
    fn short_transcript_is_one_chunk() {
        assert_eq!(
            chunks("so um  send it\nnow", MAX_INPUT_TOKENS, words),
            ["so um send it now"]
        );
    }

    #[test]
    fn empty_transcript_has_no_chunks() {
        assert!(chunks("  \n ", MAX_INPUT_TOKENS, words).is_empty());
    }

    #[test]
    fn long_transcript_splits_at_sentence_ends() {
        let text = "One two three. Four five six! Seven eight? Nine ten.";
        let chunks = chunks(text, 6, words);

        assert_eq!(
            chunks,
            ["One two three. Four five six!", "Seven eight? Nine ten."]
        );
        assert_eq!(chunks.join(" "), text);
    }

    #[test]
    fn overlong_sentence_splits_at_spaces() {
        let text = "Short one. a b c d e f g h i j. End.";
        let chunks = chunks(text, 4, words);

        assert_eq!(chunks, ["Short one.", "a b c d", "e f g h", "i j.", "End."]);
        assert!(chunks.iter().all(|chunk| words(chunk) <= 4));
        assert_eq!(chunks.join(" "), text);
    }

    #[test]
    fn default_chunk_limit_is_about_a_thousand_tokens() {
        assert_eq!(MAX_INPUT_TOKENS, 1000);
        let sentence = "this is a sentence of exactly ten words in it.";
        let text = vec![sentence; 250].join(" ");
        let chunks = chunks(&text, MAX_INPUT_TOKENS, words);

        assert_eq!(chunks.len(), 3);
        assert!(chunks.iter().all(|chunk| words(chunk) <= 1000));
        assert_eq!(chunks.join(" "), text);
    }

    #[test]
    fn sentence_ends_need_a_space_after() {
        assert_eq!(
            sentences("It costs 4.50 today. Ok… fine"),
            ["It costs 4.50 today.", "Ok…", "fine"]
        );
    }
}
