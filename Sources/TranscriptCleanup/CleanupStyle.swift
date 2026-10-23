/// The settings S1-mini was trained on. Only these values are valid in its control line; the raw
/// values are the exact spellings from the model card.
public struct CleanupStyle: Sendable, Equatable, Hashable {
    public enum Styling: String, Sendable, CaseIterable, Hashable {
        case casual
        case semiCasual = "semi-casual"
        case semiFormal = "semi-formal"
        case formal
    }

    public enum Structure: String, Sendable, CaseIterable, Hashable {
        case prose
        case lists
    }

    public enum Context: String, Sendable, CaseIterable, Hashable {
        case general
        case email
    }

    public var styling: Styling
    public var structure: Structure
    public var context: Context

    /// The model card's defaults: semi-formal, prose, general.
    public init(styling: Styling = .semiFormal, structure: Structure = .prose, context: Context = .general) {
        self.styling = styling
        self.structure = structure
        self.context = context
    }

    public static let `default` = CleanupStyle()
}
