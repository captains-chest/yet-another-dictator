//! Port of `DictationPrompt.swift`.

/// A short, well-punctuated "previous text" given to Whisper as its initial prompt.
///
/// Whisper continues the style of the text it is conditioned on. Without a prompt it often leaves
/// a short dictation without its final full stop or question mark. A few complete, punctuated
/// sentences fix that. The prompt only sets punctuation style; the language is chosen separately.
pub const TEXT: &str =
    "Hej, hvordan går det? Det går godt, tak. Hello, how are you? I'm fine, thanks.";

/// Whisper sometimes repeats its prompt when the audio holds no speech. Returns true when the
/// transcript is such an echo: a run of at least half the prompt's words, word for word. A short
/// phrase that merely also appears in the prompt ("Tak.", "Hello, how are you?") is kept.
pub fn is_echo(transcript: &str) -> bool {
    let spoken = words(transcript);
    let prompt = words(TEXT);
    if spoken.len() * 2 < prompt.len() || spoken.len() > prompt.len() {
        return false;
    }
    prompt
        .windows(spoken.len())
        .any(|window| window == spoken.as_slice())
}

fn words(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphabetic() && !c.is_numeric())
        .filter(|w| !w.is_empty())
        .map(String::from)
        .collect()
}
