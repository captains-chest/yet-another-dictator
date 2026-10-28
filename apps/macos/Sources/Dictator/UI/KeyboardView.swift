import DictationCore
import SwiftUI

/// A Mac keyboard drawing that lights up the given keys.
struct KeyboardView: View {
    let highlighted: Set<UInt16>

    private static let unit: CGFloat = 26
    private static let gap: CGFloat = 3
    private static let height = unit - gap

    var body: some View {
        let rows = Self.rows(iso: KeyboardLayout.isISO)
        let drawn = Set(rows.flatMap { $0.compactMap(\.code) } + Self.arrowCodes)
        let offBoard = highlighted.subtracting(drawn).sorted()

        VStack(alignment: .leading, spacing: 6) {
            VStack(alignment: .leading, spacing: Self.gap) {
                ForEach(rows.indices, id: \.self) { index in
                    HStack(alignment: .bottom, spacing: Self.gap) {
                        ForEach(rows[index].indices, id: \.self) { slot in
                            cap(rows[index][slot], height: index == 0 ? Self.height * 0.7 : Self.height)
                        }
                        if index == rows.count - 1 { arrows }
                    }
                }
            }
            .padding(6)
            .background(.quaternary.opacity(0.5), in: RoundedRectangle(cornerRadius: 8))
            .animation(.easeOut(duration: 0.08), value: highlighted)

            if !offBoard.isEmpty {
                Text("Also: " + offBoard.map(Self.label).joined(separator: ", "))
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("Keyboard")
        .accessibilityValue(highlighted.isEmpty ? "No keys" : highlighted.sorted().map(Self.label).joined(separator: ", "))
    }

    private var arrows: some View {
        let half = (Self.height - Self.gap) / 2
        return HStack(alignment: .bottom, spacing: Self.gap) {
            cap(Slot(0x7B, label: "←"), height: half)
            VStack(spacing: Self.gap) {
                cap(Slot(0x7E, label: "↑"), height: half)
                cap(Slot(0x7D, label: "↓"), height: half)
            }
            cap(Slot(0x7C, label: "→"), height: half)
        }
    }

    private func cap(_ slot: Slot, height: CGFloat) -> some View {
        let lit = slot.code.map(highlighted.contains) ?? false
        let label = slot.label ?? slot.code.map(Self.label) ?? ""
        return RoundedRectangle(cornerRadius: 4)
            .fill(lit ? AnyShapeStyle(.tint) : AnyShapeStyle(Color(nsColor: .controlBackgroundColor)))
            .strokeBorder(lit ? AnyShapeStyle(.tint) : AnyShapeStyle(.separator), lineWidth: 0.5)
            .overlay {
                Text(label)
                    .font(.system(size: 9, weight: .medium))
                    .lineLimit(1)
                    .minimumScaleFactor(0.6)
                    .foregroundStyle(lit ? AnyShapeStyle(.white) : AnyShapeStyle(.secondary))
            }
            .frame(width: slot.width * Self.unit - Self.gap, height: height)
            .opacity(slot.code == nil ? 0 : 1)
    }

    private static func label(_ code: UInt16) -> String {
        if KeyCode.isModifier(code) { return KeyLabels.sidedName(forModifier: code) }
        return KeyLabels.label(forKeyCode: code, typed: KeyboardLayout.character(for: code))
    }

    // MARK: - Layout (rows are 15 units wide; the arrow cluster closes the bottom row)

    private struct Slot {
        let code: UInt16?
        let width: Double
        let label: String?

        init(_ code: UInt16?, _ width: Double = 1, label: String? = nil) {
            self.code = code
            self.width = width
            self.label = label
        }
    }

    private static let arrowCodes: [UInt16] = [0x7B, 0x7C, 0x7D, 0x7E]

    private static func keys(_ codes: [UInt16]) -> [Slot] { codes.map { Slot($0) } }

    private static func rows(iso: Bool) -> [[Slot]] {
        let functionKeys: [UInt16] = [0x7A, 0x78, 0x63, 0x76, 0x60, 0x61, 0x62, 0x64, 0x65, 0x6D, 0x67, 0x6F]
        return [
            [Slot(0x35, 1.5, label: "esc")] + keys(functionKeys) + [Slot(nil, 1.5)],
            keys([iso ? 0x0A : 0x32, 0x12, 0x13, 0x14, 0x15, 0x17, 0x16, 0x1A, 0x1C, 0x19, 0x1D, 0x1B, 0x18])
                + [Slot(0x33, 2, label: "⌫")],
            [Slot(0x30, 1.5, label: "⇥")]
                + keys([0x0C, 0x0D, 0x0E, 0x0F, 0x11, 0x10, 0x20, 0x22, 0x1F, 0x23, 0x21, 0x1E])
                + [iso ? Slot(0x24, 1.5, label: "↩") : Slot(0x2A, 1.5)],
            [Slot(KeyCode.capsLock, 1.75, label: "⇪")]
                + keys([0x00, 0x01, 0x02, 0x03, 0x05, 0x04, 0x26, 0x28, 0x25, 0x29, 0x27])
                + (iso ? [Slot(0x2A), Slot(0x24, 1.25, label: "")] : [Slot(0x24, 2.25, label: "↩")]),
            (iso ? [Slot(KeyCode.leftShift, 1.25, label: "⇧"), Slot(0x32)] : [Slot(KeyCode.leftShift, 2.25, label: "⇧")])
                + keys([0x06, 0x07, 0x08, 0x09, 0x0B, 0x2D, 0x2E, 0x2B, 0x2F, 0x2C])
                + [Slot(KeyCode.rightShift, 2.75, label: "⇧")],
            [
                Slot(KeyCode.function, label: "fn"),
                Slot(KeyCode.leftControl, label: "⌃"),
                Slot(KeyCode.leftOption, label: "⌥"),
                Slot(KeyCode.leftCommand, 1.25, label: "⌘"),
                Slot(0x31, 5.5, label: ""),
                Slot(KeyCode.rightCommand, 1.25, label: "⌘"),
                Slot(KeyCode.rightOption, label: "⌥"),
            ],
        ]
    }
}
