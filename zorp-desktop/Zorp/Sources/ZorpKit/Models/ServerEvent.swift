import Foundation

public struct ConditionFrame: Codable, Hashable, Sendable {
    public let key: String
    public let value: String

    public init(key: String, value: String) {
        self.key = key
        self.value = value
    }
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

    public init(metricKey: String, expectedValue: Double, intervalLow: Double, intervalHigh: Double, confidence: Double) {
        self.metricKey = metricKey
        self.expectedValue = expectedValue
        self.intervalLow = intervalLow
        self.intervalHigh = intervalHigh
        self.confidence = confidence
    }
}

public struct MetricFrame: Codable, Hashable, Sendable {
    public let key: String
    public let value: String

    public init(key: String, value: String) {
        self.key = key
        self.value = value
    }
}

public struct ExperimentFrame: Codable, Hashable, Sendable, Identifiable {
    public var id: String { "\(status)-\(conditions.first?.value ?? "")" }
    public let status: String
    public let conditions: [ConditionFrame]
    public let expectations: [ExpectationFrame]
    public let metrics: [MetricFrame]

    public init(status: String, conditions: [ConditionFrame], expectations: [ExpectationFrame], metrics: [MetricFrame]) {
        self.status = status
        self.conditions = conditions
        self.expectations = expectations
        self.metrics = metrics
    }
}

public struct LedgerFrame: Codable, Hashable, Sendable {
    public let trackId: String
    public let present: Bool?
    public let forecasting: Bool?
    public let experiments: [ExperimentFrame]

    enum CodingKeys: String, CodingKey {
        case trackId = "track_id"
        case present
        case forecasting
        case experiments
    }

    public init(trackId: String, present: Bool?, forecasting: Bool?, experiments: [ExperimentFrame]) {
        self.trackId = trackId
        self.present = present
        self.forecasting = forecasting
        self.experiments = experiments
    }
}

public struct PanelFindingFrame: Codable, Hashable, Sendable {
    public let severity: String
    public let claim: String
    public let locus: String

    public init(severity: String, claim: String, locus: String) {
        self.severity = severity
        self.claim = claim
        self.locus = locus
    }
}

public struct AgreementFrame: Codable, Hashable, Sendable {
    public let locus: String
    public let lenses: [String]
    public let highest: String

    public init(locus: String, lenses: [String], highest: String) {
        self.locus = locus
        self.lenses = lenses
        self.highest = highest
    }
}

public enum ServerEventKind: Sendable {
    case working
    case workingDone
    case tool(name: String, summary: String, phrase: String?)
    case toolStarted(name: String, phrase: String?)
    case verify(command: String, passed: Bool)
    case notice(text: String)
    case assistantDelta(text: String)
    case assistantWithdrawn(events: Int, reask: Int, bound: Int)
    case assistant(text: String)
    case approvalRequest(id: String, tool: String, arguments: String)
    case checkpointRequest(id: String, kind: String, prompt: String)
    case investigateProgress(phase: String, attempt: Int?, of: Int?, ledger: LedgerFrame?)
    case investigateDone(trackId: String, approved: Bool?, needsPrereg: Bool, artifact: String?)
    case reviewerStarted(lens: String)
    case reviewerFinished(lens: String, findings: [PanelFindingFrame], answer: String)
    case reviewerFailed(lens: String, why: String)
    case panelDone(target: String, lensesRequested: Int, verdicts: Int, complete: Bool, agreements: [AgreementFrame])
    case context(usedTokens: UInt64, limitTokens: UInt64?, source: String)
    case compacting(messages: Int, tokensBefore: UInt64, manual: Bool)
    case compacted(ok: Bool, tokensBefore: UInt64, tokensAfter: UInt64, summary: String?)
    case unknown(type: String)
}

public struct ServerEvent: Sendable {
    public let seq: UInt64
    public let kind: ServerEventKind

    public init(seq: UInt64, kind: ServerEventKind) {
        self.seq = seq
        self.kind = kind
    }
}
