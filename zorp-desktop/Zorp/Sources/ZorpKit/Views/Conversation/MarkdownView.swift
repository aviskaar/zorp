import AppKit
import SwiftUI

/// The block structure of an answer: headings, paragraphs, lists, quotes,
/// fenced code and tables. Inline marks inside a block go through
/// Foundation's own Markdown parser, which builds attributed text and never
/// markup, so nothing a model writes can become anything but text.
public enum MarkdownBlock: Equatable, Sendable {
    case heading(level: Int, text: String)
    case paragraph(String)
    case bullet(items: [String])
    case numbered(start: Int, items: [String])
    case quote(String)
    case code(language: String?, text: String)
    case table(header: [String], rows: [[String]])
    case rule

    public static func parse(_ source: String) -> [MarkdownBlock] {
        var blocks: [MarkdownBlock] = []
        let lines = source.replacingOccurrences(of: "\r\n", with: "\n").components(separatedBy: "\n")
        var i = 0
        var paragraph: [String] = []
        func flush() {
            if !paragraph.isEmpty {
                blocks.append(.paragraph(paragraph.joined(separator: "\n")))
                paragraph = []
            }
        }
        func trimmed(_ s: String) -> String { s.trimmingCharacters(in: .whitespaces) }
        func bulletBody(_ line: String) -> String? {
            let t = trimmed(line)
            for marker in ["- ", "* ", "+ "] where t.hasPrefix(marker) { return String(t.dropFirst(2)) }
            return nil
        }
        func numberedBody(_ line: String) -> (Int, String)? {
            let t = trimmed(line)
            guard let dot = t.firstIndex(where: { $0 == "." || $0 == ")" }) else { return nil }
            let number = t[t.startIndex..<dot]
            guard !number.isEmpty, number.count <= 9, let n = Int(number) else { return nil }
            let rest = t[t.index(after: dot)...]
            guard rest.first == " " else { return nil }
            return (n, String(rest.dropFirst()))
        }
        func cells(_ line: String) -> [String] {
            var t = trimmed(line)
            if t.hasPrefix("|") { t.removeFirst() }
            if t.hasSuffix("|") { t.removeLast() }
            return t.components(separatedBy: "|").map(trimmed)
        }
        func isDivider(_ line: String) -> Bool {
            let t = trimmed(line)
            return t.contains("-") && t.allSatisfy { "|-: ".contains($0) }
        }

        while i < lines.count {
            let line = lines[i]
            let t = trimmed(line)
            if t.hasPrefix("```") || t.hasPrefix("~~~") {
                flush()
                let fence = String(t.prefix(3))
                let language = String(t.dropFirst(3)).trimmingCharacters(in: .whitespaces)
                var body: [String] = []
                i += 1
                while i < lines.count, !trimmed(lines[i]).hasPrefix(fence) {
                    body.append(lines[i])
                    i += 1
                }
                blocks.append(.code(language: language.isEmpty ? nil : language, text: body.joined(separator: "\n")))
                i += 1
                continue
            }
            if t.isEmpty {
                flush()
                i += 1
                continue
            }
            if t.hasPrefix("#") {
                let level = t.prefix(while: { $0 == "#" }).count
                if level <= 6, t.dropFirst(level).first == " " {
                    flush()
                    blocks.append(.heading(level: level, text: String(t.dropFirst(level + 1))))
                    i += 1
                    continue
                }
            }
            if t == "---" || t == "***" || t == "___" {
                flush()
                blocks.append(.rule)
                i += 1
                continue
            }
            if t.hasPrefix(">") {
                flush()
                var quoted: [String] = []
                while i < lines.count, trimmed(lines[i]).hasPrefix(">") {
                    quoted.append(String(trimmed(lines[i]).dropFirst()).trimmingCharacters(in: .whitespaces))
                    i += 1
                }
                blocks.append(.quote(quoted.joined(separator: "\n")))
                continue
            }
            if t.hasPrefix("|"), i + 1 < lines.count, isDivider(lines[i + 1]) {
                flush()
                let header = cells(line)
                var rows: [[String]] = []
                i += 2
                while i < lines.count, trimmed(lines[i]).hasPrefix("|") {
                    rows.append(cells(lines[i]))
                    i += 1
                }
                blocks.append(.table(header: header, rows: rows))
                continue
            }
            if bulletBody(line) != nil {
                flush()
                var items: [String] = []
                while i < lines.count {
                    if let body = bulletBody(lines[i]) {
                        items.append(body)
                    } else if !trimmed(lines[i]).isEmpty, lines[i].hasPrefix("  "), !items.isEmpty {
                        items[items.count - 1] += "\n" + trimmed(lines[i])
                    } else {
                        break
                    }
                    i += 1
                }
                blocks.append(.bullet(items: items))
                continue
            }
            if let first = numberedBody(line) {
                let start = first.0
                flush()
                var items: [String] = []
                while i < lines.count {
                    if let entry = numberedBody(lines[i]) {
                        items.append(entry.1)
                    } else if !trimmed(lines[i]).isEmpty, lines[i].hasPrefix("  "), !items.isEmpty {
                        items[items.count - 1] += "\n" + trimmed(lines[i])
                    } else {
                        break
                    }
                    i += 1
                }
                blocks.append(.numbered(start: start, items: items))
                continue
            }
            paragraph.append(line)
            i += 1
        }
        flush()
        return blocks
    }
}

/// Inline Markdown as attributed text: bold, italic, code spans and links.
func inline(_ text: String) -> AttributedString {
    let options = AttributedString.MarkdownParsingOptions(interpretedSyntax: .inlineOnlyPreservingWhitespace)
    return (try? AttributedString(markdown: text, options: options)) ?? AttributedString(text)
}

public struct MarkdownView: View {
    public let text: String
    private let blocks: [MarkdownBlock]

    public init(_ text: String) {
        self.text = text
        self.blocks = MarkdownBlock.parse(text)
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            ForEach(Array(blocks.enumerated()), id: \.offset) { _, block in
                view(for: block)
            }
        }
        .textSelection(.enabled)
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    @ViewBuilder
    private func view(for block: MarkdownBlock) -> some View {
        switch block {
        case .heading(let level, let text):
            Text(inline(text))
                .font(level == 1 ? .title2 : level == 2 ? .title3 : .headline)
                .padding(.top, 4)
        case .paragraph(let text):
            Text(inline(text)).lineSpacing(3)
        case .bullet(let items):
            VStack(alignment: .leading, spacing: 4) {
                ForEach(Array(items.enumerated()), id: \.offset) { _, item in
                    HStack(alignment: .firstTextBaseline, spacing: 8) {
                        Text("•").foregroundStyle(.secondary)
                        Text(inline(item))
                    }
                }
            }
        case .numbered(let start, let items):
            VStack(alignment: .leading, spacing: 4) {
                ForEach(Array(items.enumerated()), id: \.offset) { offset, item in
                    HStack(alignment: .firstTextBaseline, spacing: 8) {
                        Text("\(start + offset).").foregroundStyle(.secondary).monospacedDigit()
                        Text(inline(item))
                    }
                }
            }
        case .quote(let text):
            HStack(spacing: 10) {
                Rectangle().fill(Color.secondary.opacity(0.4)).frame(width: 3)
                Text(inline(text)).foregroundStyle(.secondary)
            }
        case .code(let language, let text):
            CodeBlockView(language: language, text: text)
        case .table(let header, let rows):
            TableBlockView(header: header, rows: rows)
        case .rule:
            Divider()
        }
    }
}

struct CodeBlockView: View {
    let language: String?
    let text: String
    @State private var copied = false

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack {
                Text(language ?? "code").font(.caption2).foregroundStyle(.secondary)
                Spacer()
                Button {
                    NSPasteboard.general.clearContents()
                    NSPasteboard.general.setString(text, forType: .string)
                    copied = true
                    DispatchQueue.main.asyncAfter(deadline: .now() + 1.5) { copied = false }
                } label: {
                    Label(copied ? "Copied" : "Copy", systemImage: copied ? "checkmark" : "doc.on.doc")
                        .font(.caption2)
                }
                .buttonStyle(.borderless)
            }
            .padding(.horizontal, 10)
            .padding(.vertical, 5)
            Divider()
            ScrollView(.horizontal) {
                Text(text)
                    .font(.system(.callout, design: .monospaced))
                    .padding(10)
                    .fixedSize(horizontal: true, vertical: false)
            }
        }
        .background(Color(nsColor: .textBackgroundColor).opacity(0.6))
        .clipShape(RoundedRectangle(cornerRadius: 8))
        .overlay(RoundedRectangle(cornerRadius: 8).stroke(Color.secondary.opacity(0.2)))
    }
}

struct TableBlockView: View {
    let header: [String]
    let rows: [[String]]

    var body: some View {
        ScrollView(.horizontal) {
            Grid(alignment: .leading, horizontalSpacing: 16, verticalSpacing: 6) {
                GridRow {
                    ForEach(Array(header.enumerated()), id: \.offset) { _, cell in
                        Text(inline(cell)).bold()
                    }
                }
                Divider()
                ForEach(Array(rows.enumerated()), id: \.offset) { _, row in
                    GridRow {
                        ForEach(Array(row.enumerated()), id: \.offset) { _, cell in
                            Text(inline(cell))
                        }
                    }
                }
            }
            .padding(10)
        }
        .background(Color(nsColor: .textBackgroundColor).opacity(0.4))
        .clipShape(RoundedRectangle(cornerRadius: 8))
    }
}
