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
        /// A review panel: one row per reviewer as it reports, and the
        /// agreement counted in code once it is done.
        case panel(PanelRun)
        /// A bolt run: the phase in a sentence, the ledger as recorded.
        case investigate(InvestigateRun)
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

public struct PanelReviewer: Equatable, Sendable {
    public enum State: Equatable, Sendable { case running, finished, failed }
    public let lens: String
    public var state: State
    public var findings: [PanelFindingFrame]
    public var answer: String
    public var why: String?
}

public struct PanelRun: Equatable, Sendable {
    public var reviewers: [PanelReviewer] = []
    public var target: String?
    public var lensesRequested: Int?
    public var verdicts: Int?
    public var complete: Bool?
    public var agreements: [AgreementFrame] = []

    public var isDone: Bool { target != nil }
}

public struct InvestigateRun: Equatable, Sendable {
    public var question: String?
    public var line: String = "Starting."
    public var attempt: Int?
    public var of: Int?
    public var ledger: LedgerFrame?
    public var verdict: String?
    public var artifact: String?
    public var needsPrereg = false
    public var finished = false

    /// What the run is doing, in a sentence. The server sends a phase from
    /// a closed set and the words are chosen here, as `web/src/zorp-mode.ts`
    /// chooses them for the browser.
    public static func phaseLine(_ phase: String, attempt: Int?, of: Int?) -> String {
        let a = attempt.map(String.init) ?? "?"
        let n = of.map(String.init) ?? "?"
        switch phase {
        case "prereg": return "Committing the pre-registration. It cannot be changed after this."
        case "attempt-started": return "Attempt \(a) of \(n)."
        case "attempt-finished": return "Attempt \(a) of \(n) finished and is recorded."
        case "write-up": return "The attempts are done. Writing the track up."
        case "critique": return "Auditing the draft against what the attempts recorded."
        default: return "Running."
        }
    }

    public static func verdictLine(_ approved: Bool?) -> String {
        switch approved {
        case true?: return "The attempt finished and the track stays active."
        case false?: return "The attempt finished and the track was killed. Either the pre-registered kill threshold was breached, or the checkpoint said no."
        case nil: return "The attempt did not finish. Whatever it recorded before it stopped is below."
        }
    }
}
