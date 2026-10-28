import AppKit
import DictationCore
import SwiftUI

/// The What's New window: the bundled CHANGELOG.md sections for this update, or a welcome on
/// first launch. Everything is read from the app bundle, so no network is needed.
@MainActor
final class WhatsNewWindow {
    private var window: NSWindow?

    struct Content {
        var title: String
        var intro: String?
        var entries: [ChangelogEntry]
    }

    static var currentVersion: AppVersion? {
        (Bundle.main.infoDictionary?["CFBundleShortVersionString"] as? String).flatMap(AppVersion.init)
    }

    static func bundledChangelog() -> [ChangelogEntry] {
        guard
            let url = Bundle.main.url(forResource: "CHANGELOG", withExtension: "md"),
            let text = try? String(contentsOf: url, encoding: .utf8)
        else { return [] }
        return Changelog.parse(text)
    }

    /// Shows the notes for the current version (menu item).
    func showCurrent() {
        let entries = Self.bundledChangelog()
        let current = Self.currentVersion
        let shown = entries.filter { $0.version == current }
        show(Content(title: "What's New in Dictator", intro: nil, entries: shown.isEmpty ? Array(entries.prefix(1)) : shown))
    }

    func show(_ content: Content) {
        let view = WhatsNewView(content: content) { [weak self] in self?.window?.close() }
        if let window {
            window.contentViewController = NSHostingController(rootView: view)
        } else {
            let window = NSWindow(contentViewController: NSHostingController(rootView: view))
            window.styleMask = [.titled, .closable]
            window.isReleasedWhenClosed = false
            window.center()
            self.window = window
        }
        window?.title = content.title
        NSApp.activate()
        window?.makeKeyAndOrderFront(nil)
    }
}

private struct WhatsNewView: View {
    let content: WhatsNewWindow.Content
    let close: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: 12) {
                Image(nsImage: NSApp.applicationIconImage)
                    .resizable()
                    .frame(width: 48, height: 48)
                VStack(alignment: .leading, spacing: 2) {
                    Text(content.title).font(.title2.bold())
                    if let intro = content.intro {
                        Text(intro).foregroundStyle(.secondary)
                    }
                }
            }
            .padding(20)

            Divider()

            ScrollView {
                VStack(alignment: .leading, spacing: 18) {
                    if content.entries.isEmpty {
                        Text("No release notes are bundled with this build.").foregroundStyle(.secondary)
                    }
                    ForEach(content.entries, id: \.version) { entry in
                        VStack(alignment: .leading, spacing: 8) {
                            HStack(alignment: .firstTextBaseline) {
                                Text("Version \(entry.version.description)").font(.headline)
                                if !entry.note.isEmpty {
                                    Text(entry.note).font(.subheadline).foregroundStyle(.secondary)
                                }
                            }
                            MarkdownBlock(markdown: entry.body)
                        }
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(20)
            }
            .frame(maxHeight: 360)

            Divider()

            HStack {
                Spacer()
                Button("Close", action: close)
                    .keyboardShortcut(.defaultAction)
            }
            .padding(16)
        }
        .frame(width: 440)
        .fixedSize(horizontal: false, vertical: true)
    }
}

/// Renders the small Markdown subset the changelog uses: `###` headings, `-` bullets, paragraphs,
/// and inline emphasis, code and links.
private struct MarkdownBlock: View {
    let markdown: String

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            ForEach(Array(lines.enumerated()), id: \.offset) { _, line in
                if line.hasPrefix("### ") {
                    Text(Self.inline(String(line.dropFirst(4))))
                        .font(.subheadline.weight(.semibold))
                        .foregroundStyle(.secondary)
                        .padding(.top, 4)
                } else if line.hasPrefix("- ") || line.hasPrefix("* ") {
                    HStack(alignment: .firstTextBaseline, spacing: 6) {
                        Text("•")
                        Text(Self.inline(String(line.dropFirst(2))))
                            .fixedSize(horizontal: false, vertical: true)
                    }
                } else {
                    Text(Self.inline(line)).fixedSize(horizontal: false, vertical: true)
                }
            }
        }
    }

    private var lines: [String] {
        markdown.split(separator: "\n").map { $0.trimmingCharacters(in: .whitespaces) }.filter { !$0.isEmpty }
    }

    private static func inline(_ text: String) -> AttributedString {
        (try? AttributedString(markdown: text, options: .init(interpretedSyntax: .inlineOnlyPreservingWhitespace)))
            ?? AttributedString(text)
    }
}
