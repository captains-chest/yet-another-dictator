import ApplicationServices

/// Reads the text just before the cursor in the focused field of the frontmost app, through the
/// Accessibility API, so a dictation can be spaced and capitalized to fit.
///
/// Best effort: returns nil when the app doesn't expose its text, Accessibility isn't granted, or
/// the App Sandbox blocks reading other apps (the shipped sandboxed build), and the caller falls
/// back to what Dictator typed itself. Password fields are never read. The text is used once and
/// never stored or logged.
enum FocusedText {
    @MainActor
    static func textBeforeCursor(maxLength: Int = 64) -> String? {
        guard AXIsProcessTrusted() else { return nil }
        let system = AXUIElementCreateSystemWide()
        // A hung app must not delay typing for long.
        AXUIElementSetMessagingTimeout(system, 0.1)

        var focused: CFTypeRef?
        guard
            AXUIElementCopyAttributeValue(system, kAXFocusedUIElementAttribute as CFString, &focused) == .success,
            let focused, CFGetTypeID(focused) == AXUIElementGetTypeID()
        else { return nil }
        let element = focused as! AXUIElement
        AXUIElementSetMessagingTimeout(element, 0.1)

        var subrole: CFTypeRef?
        if AXUIElementCopyAttributeValue(element, kAXSubroleAttribute as CFString, &subrole) == .success,
           (subrole as? String) == kAXSecureTextFieldSubrole {
            return nil
        }

        var rangeValue: CFTypeRef?
        guard
            AXUIElementCopyAttributeValue(element, kAXSelectedTextRangeAttribute as CFString, &rangeValue) == .success,
            let rangeValue, CFGetTypeID(rangeValue) == AXValueGetTypeID()
        else { return nil }
        var selection = CFRange()
        guard AXValueGetValue(rangeValue as! AXValue, .cfRange, &selection), selection.location >= 0 else { return nil }

        let start = max(0, selection.location - maxLength)
        var before = CFRange(location: start, length: selection.location - start)
        guard before.length > 0 else { return "" }
        guard let parameter = AXValueCreate(.cfRange, &before) else { return nil }
        var text: CFTypeRef?
        guard AXUIElementCopyParameterizedAttributeValue(
            element, kAXStringForRangeParameterizedAttribute as CFString, parameter, &text
        ) == .success else { return nil }
        return text as? String
    }
}
