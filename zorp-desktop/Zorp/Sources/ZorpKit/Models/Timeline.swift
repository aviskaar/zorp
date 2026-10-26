import Foundation

/// One thing drawn in a conversation, in the order it happened. A replayed
/// transcript and a live turn produce the same items, so a reopened
/// conversation looks the way it did while it ran.
public struct TimelineItem: Identifiable, Equatable, Sendable {
    public enum Kind: Equatable, Sendable {
        case user(text: String)
        /// An answer. `ordinal` counts answers from 1, the way the server
        /// counts them for branching. Model-authored text.
        case assistant(text: String, streaming: Bool, ordinal: Int?)
        case tool(name: String, phrase: String?, summary: String?, state: ActivityLine.State)
        case verify(command: String, passed: Bool)
        case notice(text: String)
        case error(text: String)
        case stopped
        /// A tool call waiting on a person. `decision` is nil while it waits.
        case approval(tool: String, arguments: String, decision: Bool?)
        case checkpoint(kind: String, prompt: String, decision: Bool?)
        case compacting(messages: Int)
        case compaction(messages: Int, tokensBefore: Int64, tokensAfter: Int64, summary: String?, ok: Bool, reason: String?)
        case memory(citations: [MemoryCitation], unavailable: String?)
    }

    public let id: UUID
    public var kind: Kind

    public init(id: UUID = UUID(), _ kind: Kind) {
        self.id = id
        self.kind = kind
    }

    public var isTool: Bool {
        if case .tool = kind { return true }
        return false
    }
}

extension TimelineItem {
    /// The items a stored transcript replays as, with each compaction
    /// marker after the entry the server counted it to.
    public static func replay(_ transcript: Transcript) -> [TimelineItem] {
        var items: [TimelineItem] = []
        var pending = transcript.compactions.sorted { $0.after < $1.after }
        var answers = 0
        func markers(upTo drawn: Int) {
            while let next = pending.first, next.after <= drawn {
                pending.removeFirst()
                items.append(TimelineItem(.compaction(
                    messages: next.messages, tokensBefore: next.tokensBefore, tokensAfter: next.tokensAfter,
                    summary: next.summary, ok: true, reason: nil)))
            }
        }
        for (index, entry) in transcript.messages.enumerated() {
            switch entry.role {
            case "tool":
                let summary = entry.summary ?? ""
                items.append(TimelineItem(.tool(
                    name: entry.name ?? "tool", phrase: entry.phrase, summary: summary,
                    state: ActivityLine.state(for: summary))))
            case "user":
                items.append(TimelineItem(.user(text: entry.content ?? "")))
            default:
                answers += 1
                items.append(TimelineItem(.assistant(text: entry.content ?? "", streaming: false, ordinal: answers)))
            }
            markers(upTo: index + 1)
        }
        markers(upTo: Int.max)
        return items
    }
}
