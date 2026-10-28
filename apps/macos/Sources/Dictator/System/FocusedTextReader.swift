import DictationCore

/// The controller's `PrecedingTextReading` port, backed by the Accessibility API through
/// `FocusedText`. Returns nil whenever the field can't be read (see `FocusedText`), and the
/// controller falls back to what Dictator typed itself.
@MainActor
final class FocusedTextReader: PrecedingTextReading {
    func textBeforeCursor(maxLength: Int) -> String? {
        FocusedText.textBeforeCursor(maxLength: maxLength)
    }
}
