import Foundation

// One frame of `GET /api/sessions/:id/events`. The server serializes
// `zorp_web::event::Event`: a `seq` that climbs for the life of the session
// and a `type` naming one of the `EventKind` variants, with that variant's
// fields beside it. Every variant the server can send is named here, so a
// frame this app does not draw is a decision and not an accident.

public struct ConditionFrame: Codable, Hashable, Sendable {
    public let key: String
    public let value: String
}

public struct ExpectationFrame: Codable, Hashable, Sendable {
    public let metricKey: String
    public let expectedValue: Double
    public let intervalLow: Double
    public let intervalHigh: Double
    public let confidence: Double

    enum CodingKeys: String, CodingKey {
        case metricKey = "metric_key"
        case expectedValue = "expected_value"
        case intervalLow = "interval_low"
        case intervalHigh = "interval_high"
        case confidence
    }
}

public struct MetricFrame: Codable, Hashable, Sendable {
    public let key: String
    public let value: String
}

public struct ExperimentFrame: Codable, Hashable, Sendable, Identifiable {
    public let id: String
    public let status: String
    public let conditions: [ConditionFrame]
    public let expectations: [ExpectationFrame]
    public let metrics: [MetricFrame]
}

public struct LedgerFrame: Codable, Hashable, Sendable {
    public let trackId: String
    public let present: Bool
    public let forecasting: Bool
    public let experiments: [ExperimentFrame]

    enum CodingKeys: String, CodingKey {
        case trackId = "track_id"
        case present, forecasting, experiments
    }
}

public struct PanelFindingFrame: Codable, Hashable, Sendable {
    /// `note`, `concern` or `blocking`.
    public let severity: String
    /// What the reviewer said. Model-authored text.
    public let claim: String
    public let locus: String
}

public struct AgreementFrame: Codable, Hashable, Sendable {
    public let locus: String
    public let lenses: [String]
    public let highest: String
}

public struct MemoryCitation: Codable, Hashable, Sendable {
    public let conversationId: String
    public let title: String
    public let seq: Int64
    /// `user` or `assistant`. An assistant line is model-authored text.
    public let author: String
    public let when: String
    public let text: String
    public let score: Double

    enum CodingKeys: String, CodingKey {
        case title, seq, author, when, text, score
        case conversationId = "conversation_id"
    }
}

public enum ServerEventKind: Hashable, Sendable {
    case working
    case workingDone
    case toolStarted(name: String, phrase: String?)
    case tool(name: String, summary: String, phrase: String?)
    case verify(command: String, passed: Bool)
    case notice(text: String)
    case assistantDelta(text: String)
    case assistantWithdrawn(events: Int, reask: Int, bound: Int)
    case assistant(text: String)
    case approvalRequest(id: String, tool: String, arguments: String)
    case context(usedTokens: Int64, limitTokens: Int64?, source: String)
    case compacting(messages: Int, tokensBefore: Int64, manual: Bool)
    case compacted(ok: Bool, tokensBefore: Int64, tokensAfter: Int64, summary: String?, reason: String?, manual: Bool)
    case reviewerStarted(lens: String)
    case reviewerFinished(lens: String, findings: [PanelFindingFrame], answer: String)
    case reviewerFailed(lens: String, why: String)
    case panelDone(target: String, lensesRequested: Int, verdicts: Int, complete: Bool, agreements: [AgreementFrame])
    case investigateProgress(trackId: String, phase: String, attempt: Int?, of: Int?, ledger: LedgerFrame?)
    case investigateDone(trackId: String, approved: Bool?, needsPrereg: Bool, artifact: String?)
    case checkpointRequest(id: String, kind: String, prompt: String)
    case memory(used: [MemoryCitation], unavailable: String?)
    case sessionTitle(title: String)
    case error(message: String)
    case stopped
    case done
    /// A `type` this build does not know. Kept rather than dropped so a
    /// newer server is visible as such.
    case unknown(type: String)
}

public struct ServerEvent: Hashable, Sendable {
    public let seq: UInt64
    public let kind: ServerEventKind

    public init(seq: UInt64, kind: ServerEventKind) {
        self.seq = seq
        self.kind = kind
    }

    /// The kind's `type` string, as the server spells it.
    public var type: String {
        switch kind {
        case .working: return "working"
        case .workingDone: return "working_done"
        case .toolStarted: return "tool_started"
        case .tool: return "tool"
        case .verify: return "verify"
        case .notice: return "notice"
        case .assistantDelta: return "assistant_delta"
        case .assistantWithdrawn: return "assistant_withdrawn"
        case .assistant: return "assistant"
        case .approvalRequest: return "approval_request"
        case .context: return "context"
        case .compacting: return "compacting"
        case .compacted: return "compacted"
        case .reviewerStarted: return "reviewer_started"
        case .reviewerFinished: return "reviewer_finished"
        case .reviewerFailed: return "reviewer_failed"
        case .panelDone: return "panel_done"
        case .investigateProgress: return "investigate_progress"
        case .investigateDone: return "investigate_done"
        case .checkpointRequest: return "checkpoint_request"
        case .memory: return "memory"
        case .sessionTitle: return "session_title"
        case .error: return "error"
        case .stopped: return "stopped"
        case .done: return "done"
        case .unknown(let type): return type
        }
    }
}

extension ServerEvent: Decodable {
    private enum Key: String, CodingKey {
        case seq, type, name, phrase, summary, command, passed, text, events, reask, bound
        case id, tool, arguments, source, messages, manual, ok, reason, lens, findings, answer
        case why, target, agreements, verdicts, complete, phase, attempt, of, ledger, approved
        case artifact, kind, prompt, used, unavailable, title, message
        case usedTokens = "used_tokens"
        case limitTokens = "limit_tokens"
        case tokensBefore = "tokens_before"
        case tokensAfter = "tokens_after"
        case lensesRequested = "lenses_requested"
        case trackId = "track_id"
        case needsPrereg = "needs_prereg"
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: Key.self)
        seq = try c.decodeIfPresent(UInt64.self, forKey: .seq) ?? 0
        let type = try c.decode(String.self, forKey: .type)
        func s(_ k: Key) throws -> String { try c.decodeIfPresent(String.self, forKey: k) ?? "" }
        func i(_ k: Key) throws -> Int { try c.decodeIfPresent(Int.self, forKey: k) ?? 0 }
        func big(_ k: Key) throws -> Int64 { try c.decodeIfPresent(Int64.self, forKey: k) ?? 0 }
        func b(_ k: Key) throws -> Bool { try c.decodeIfPresent(Bool.self, forKey: k) ?? false }
        func opt(_ k: Key) throws -> String? { try c.decodeIfPresent(String.self, forKey: k) }

        switch type {
        case "working": kind = .working
        case "working_done": kind = .workingDone
        case "tool_started": kind = .toolStarted(name: try s(.name), phrase: try opt(.phrase))
        case "tool": kind = .tool(name: try s(.name), summary: try s(.summary), phrase: try opt(.phrase))
        case "verify": kind = .verify(command: try s(.command), passed: try b(.passed))
        case "notice": kind = .notice(text: try s(.text))
        case "assistant_delta": kind = .assistantDelta(text: try s(.text))
        case "assistant_withdrawn":
            kind = .assistantWithdrawn(events: try i(.events), reask: try i(.reask), bound: try i(.bound))
        case "assistant": kind = .assistant(text: try s(.text))
        case "approval_request":
            kind = .approvalRequest(id: try s(.id), tool: try s(.tool), arguments: try s(.arguments))
        case "context":
            kind = .context(
                usedTokens: try big(.usedTokens),
                limitTokens: try c.decodeIfPresent(Int64.self, forKey: .limitTokens),
                source: try s(.source))
        case "compacting":
            kind = .compacting(messages: try i(.messages), tokensBefore: try big(.tokensBefore), manual: try b(.manual))
        case "compacted":
            kind = .compacted(
                ok: try b(.ok), tokensBefore: try big(.tokensBefore), tokensAfter: try big(.tokensAfter),
                summary: try opt(.summary), reason: try opt(.reason), manual: try b(.manual))
        case "reviewer_started": kind = .reviewerStarted(lens: try s(.lens))
        case "reviewer_finished":
            kind = .reviewerFinished(
                lens: try s(.lens),
                findings: try c.decodeIfPresent([PanelFindingFrame].self, forKey: .findings) ?? [],
                answer: try s(.answer))
        case "reviewer_failed": kind = .reviewerFailed(lens: try s(.lens), why: try s(.why))
        case "panel_done":
            kind = .panelDone(
                target: try s(.target), lensesRequested: try i(.lensesRequested), verdicts: try i(.verdicts),
                complete: try b(.complete),
                agreements: try c.decodeIfPresent([AgreementFrame].self, forKey: .agreements) ?? [])
        case "investigate_progress":
            kind = .investigateProgress(
                trackId: try s(.trackId), phase: try s(.phase),
                attempt: try c.decodeIfPresent(Int.self, forKey: .attempt),
                of: try c.decodeIfPresent(Int.self, forKey: .of),
                ledger: try? c.decodeIfPresent(LedgerFrame.self, forKey: .ledger))
        case "investigate_done":
            kind = .investigateDone(
                trackId: try s(.trackId), approved: try c.decodeIfPresent(Bool.self, forKey: .approved),
                needsPrereg: try b(.needsPrereg), artifact: try opt(.artifact))
        case "checkpoint_request":
            kind = .checkpointRequest(id: try s(.id), kind: try s(.kind), prompt: try s(.prompt))
        case "memory":
            kind = .memory(
                used: (try? c.decodeIfPresent([MemoryCitation].self, forKey: .used)) ?? [],
                unavailable: try opt(.unavailable))
        case "session_title": kind = .sessionTitle(title: try s(.title))
        case "error": kind = .error(message: try s(.message))
        case "stopped": kind = .stopped
        case "done": kind = .done
        default: kind = .unknown(type: type)
        }
    }

    /// Decode one `data:` payload, or nothing if it is not an event.
    public static func parse(_ data: Data) -> ServerEvent? {
        try? JSONDecoder().decode(ServerEvent.self, from: data)
    }
}
