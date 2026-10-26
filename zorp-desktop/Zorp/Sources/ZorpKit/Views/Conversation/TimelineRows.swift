import AppKit
import SwiftUI

/// One row of the conversation.
struct TimelineRow: View {
    let item: TimelineItem
    let session: SessionViewModel
    let onBranch: (Int) -> Void

    var body: some View {
        switch item.kind {
        case .user(let text):
            UserMessageRow(text: text)
        case .assistant(let text, let streaming, let ordinal):
            AssistantMessageRow(text: text, streaming: streaming, ordinal: ordinal, onBranch: onBranch)
        case .tool(let name, let phrase, let summary, let state):
            ToolRow(name: name, phrase: phrase, summary: summary, state: state)
        case .verify(let command, let passed):
            Label {
                Text("Verify: \(command)").font(.system(.callout, design: .monospaced))
            } icon: {
                Image(systemName: passed ? "checkmark.seal.fill" : "xmark.seal.fill")
                    .foregroundStyle(passed ? Color.green : Color.red)
            }
        case .notice(let text):
            NoteRow(systemImage: "info.circle", text: text, isError: false)
        case .error(let text):
            NoteRow(systemImage: "exclamationmark.triangle.fill", text: text, isError: true)
        case .stopped:
            NoteRow(systemImage: "stop.circle", text: "Stopped.", isError: false)
        case .approval(let tool, let arguments, let decision):
            ApprovalCard(tool: tool, arguments: arguments, decision: decision) { allow in
                Task { await session.answerApproval(item.id, allow: allow) }
            }
        case .checkpoint(let kind, let prompt, let decision):
            CheckpointCard(kind: kind, prompt: prompt, decision: decision) { allow in
                Task { await session.answerCheckpoint(item.id, allow: allow) }
            }
        case .compacting(let messages):
            HStack(spacing: 8) {
                ProgressView().controlSize(.small)
                Text("Summarizing \(messages) earlier messages to make room…")
                    .font(.callout).foregroundStyle(.secondary)
            }
        case .compaction(let messages, let before, let after, let summary, let ok, let reason):
            CompactionRow(messages: messages, before: before, after: after, summary: summary, ok: ok, reason: reason)
        case .memory(let citations, let unavailable):
            MemoryRow(citations: citations, unavailable: unavailable)
        }
    }
}

struct UserMessageRow: View {
    let text: String

    var body: some View {
        HStack {
            Spacer(minLength: 60)
            Text(text)
                .textSelection(.enabled)
                .padding(.horizontal, 14)
                .padding(.vertical, 10)
                .background(Color.accentColor.opacity(0.14))
                .clipShape(RoundedRectangle(cornerRadius: 14))
        }
    }
}

struct AssistantMessageRow: View {
    let text: String
    let streaming: Bool
    let ordinal: Int?
    let onBranch: (Int) -> Void
    @State private var copied = false

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            MarkdownView(text)
            if streaming {
                ProgressView().controlSize(.small)
            } else {
                HStack(spacing: 12) {
                    Button {
                        NSPasteboard.general.clearContents()
                        NSPasteboard.general.setString(text, forType: .string)
                        copied = true
                        DispatchQueue.main.asyncAfter(deadline: .now() + 1.5) { copied = false }
                    } label: {
                        Label(copied ? "Copied" : "Copy", systemImage: copied ? "checkmark" : "doc.on.doc")
                    }
                    .help("Copy this answer")
                    if let ordinal {
                        Button {
                            onBranch(ordinal)
                        } label: {
                            Label("Branch", systemImage: "arrow.triangle.branch")
                        }
                        .help("Start a new conversation from here")
                    }
                }
                .buttonStyle(.borderless)
                .font(.caption)
                .foregroundStyle(.secondary)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }
}

struct ToolRow: View {
    let name: String
    let phrase: String?
    let summary: String?
    let state: ActivityLine.State
    @State private var expanded = false

    private var color: Color {
        switch state {
        case .running: return .orange
        case .ok: return .green
        case .failed: return .red
        }
    }

    var body: some View {
        let call = ActivityLine.split(name)
        let headline = ActivityLine.headline(name: name, phrase: phrase)
        VStack(alignment: .leading, spacing: 4) {
            Button {
                if call.command != nil { expanded.toggle() }
            } label: {
                HStack(spacing: 8) {
                    if state == .running {
                        ProgressView().controlSize(.mini)
                    } else {
                        Circle().fill(color).frame(width: 7, height: 7)
                    }
                    Text(headline.text)
                        .font(call.command == nil ? .system(.callout, design: .monospaced) : .callout)
                        .italic(headline.modelWritten)
                        .lineLimit(1)
                    if call.command != nil {
                        Image(systemName: expanded ? "chevron.down" : "chevron.right")
                            .font(.caption2)
                    }
                    Spacer()
                }
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .help(headline.modelWritten ? "The model's own description. Click to see the command." : (summary ?? "Running"))
            if expanded, let command = call.command {
                VStack(alignment: .leading, spacing: 4) {
                    Text(command)
                        .font(.system(.caption, design: .monospaced))
                        .textSelection(.enabled)
                    if let summary, !summary.isEmpty {
                        Text(summary).font(.caption)
                    }
                }
                .padding(8)
                .frame(maxWidth: .infinity, alignment: .leading)
                .background(Color(nsColor: .textBackgroundColor).opacity(0.6))
                .clipShape(RoundedRectangle(cornerRadius: 6))
            }
        }
        .foregroundStyle(.secondary)
    }
}

struct NoteRow: View {
    let systemImage: String
    let text: String
    let isError: Bool

    var body: some View {
        Label {
            Text(text).textSelection(.enabled)
        } icon: {
            Image(systemName: systemImage)
        }
        .font(.callout)
        .foregroundStyle(isError ? Color.red : Color.secondary)
    }
}

/// A tool call waiting on a person. Open while it waits and folded to its
/// head once settled, the way the browser's card behaves.
struct ApprovalCard: View {
    let tool: String
    let arguments: String
    let decision: Bool?
    let onDecision: (Bool) -> Void

    private var heading: String {
        switch decision {
        case nil: return "Allow this?"
        case true?: return "Allowed"
        case false?: return "Denied"
        }
    }

    private var symbol: String {
        switch decision {
        case nil: return "hand.raised.fill"
        case true?: return "checkmark.circle.fill"
        case false?: return "xmark.circle.fill"
        }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack {
                Image(systemName: symbol).foregroundStyle(decision == nil ? Color.orange : Color.secondary)
                Text(heading).font(.headline)
                Text(tool).font(.system(.callout, design: .monospaced)).foregroundStyle(.secondary)
                Spacer()
            }
            if decision == nil {
                ScrollView {
                    Text(arguments)
                        .font(.system(.caption, design: .monospaced))
                        .textSelection(.enabled)
                        .frame(maxWidth: .infinity, alignment: .leading)
                }
                .frame(maxHeight: 220)
                .padding(8)
                .background(Color(nsColor: .textBackgroundColor).opacity(0.7))
                .clipShape(RoundedRectangle(cornerRadius: 6))
                HStack {
                    Spacer()
                    Button("Deny") { onDecision(false) }
                    Button("Allow") { onDecision(true) }
                        .buttonStyle(.borderedProminent)
                        .keyboardShortcut(.return, modifiers: [.command])
                }
            }
        }
        .padding(12)
        .background(Color.orange.opacity(decision == nil ? 0.08 : 0.03))
        .clipShape(RoundedRectangle(cornerRadius: 10))
        .overlay(RoundedRectangle(cornerRadius: 10).stroke(Color.orange.opacity(decision == nil ? 0.4 : 0.15)))
    }
}

struct CheckpointCard: View {
    let kind: String
    let prompt: String
    let decision: Bool?
    let onDecision: (Bool) -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                Image(systemName: "flag.checkered")
                Text("Research checkpoint: \(kind)").font(.headline)
                Spacer()
                if let decision {
                    Text(decision ? "Approved" : "Rejected").foregroundStyle(.secondary)
                }
            }
            Text(prompt).textSelection(.enabled)
            if decision == nil {
                Text("Rejecting kills the track and is written into its record.")
                    .font(.caption).foregroundStyle(.secondary)
                HStack {
                    Spacer()
                    Button("Reject", role: .destructive) { onDecision(false) }
                    Button("Approve") { onDecision(true) }.buttonStyle(.borderedProminent)
                }
            }
        }
        .padding(12)
        .background(Color.blue.opacity(0.06))
        .clipShape(RoundedRectangle(cornerRadius: 10))
    }
}

struct CompactionRow: View {
    let messages: Int
    let before: Int64
    let after: Int64
    let summary: String?
    let ok: Bool
    let reason: String?
    @State private var expanded = false

    private var label: String {
        guard ok else { return "Could not summarize: \(reason ?? "unknown reason")" }
        let what = messages > 0 ? "\(messages) earlier messages" : "earlier messages"
        return "Summarized \(what) (\(before) → \(after) tokens)"
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack {
                Rectangle().fill(Color.secondary.opacity(0.3)).frame(height: 1)
                Button {
                    expanded.toggle()
                } label: {
                    Text(label).font(.caption)
                }
                .buttonStyle(.plain)
                .foregroundStyle(.secondary)
                Rectangle().fill(Color.secondary.opacity(0.3)).frame(height: 1)
            }
            if expanded, let summary {
                VStack(alignment: .leading, spacing: 4) {
                    Text("Model-written summary. It stands in for the older messages when the model is asked; the messages themselves are kept.")
                        .font(.caption2).foregroundStyle(.secondary)
                    Text(summary).font(.callout).textSelection(.enabled)
                }
                .padding(8)
                .background(Color.secondary.opacity(0.06))
                .clipShape(RoundedRectangle(cornerRadius: 6))
            }
        }
    }
}

struct MemoryRow: View {
    let citations: [MemoryCitation]
    let unavailable: String?
    @State private var expanded = false

    private var label: String {
        if let unavailable { return unavailable }
        return "Recalled \(citations.count) passage\(citations.count == 1 ? "" : "s") from earlier conversations"
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Button {
                expanded.toggle()
            } label: {
                Label(label, systemImage: "brain").font(.caption)
            }
            .buttonStyle(.plain)
            .foregroundStyle(.secondary)
            if expanded {
                ForEach(Array(citations.enumerated()), id: \.offset) { _, citation in
                    VStack(alignment: .leading, spacing: 2) {
                        Text("\(citation.title) · \(citation.author == "assistant" ? "model output" : "you") · \(citation.when)")
                            .font(.caption2).foregroundStyle(.secondary)
                        Text(citation.text).font(.callout).textSelection(.enabled)
                    }
                    .padding(8)
                    .background(Color.secondary.opacity(0.06))
                    .clipShape(RoundedRectangle(cornerRadius: 6))
                }
            }
        }
    }
}
