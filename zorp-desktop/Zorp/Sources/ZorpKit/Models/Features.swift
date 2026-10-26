import Foundation

// The shapes behind the optional features: skills, agents, voice, the
// review panel, investigate runs, conversation search, MCP, the data on
// disk and the doctor. Each mirrors its interface in `web/src/api.ts`.

/// `GET /api/skills`.
public struct SkillListing: Codable, Sendable {
    public let skills: [SkillSummary]
    public let warnings: [String]
    public let offer: SkillOffer?

    enum CodingKeys: String, CodingKey { case skills, warnings, offer }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        skills = (try? c.decodeIfPresent([SkillSummary].self, forKey: .skills)) ?? []
        warnings = (try? c.decodeIfPresent([String].self, forKey: .warnings)) ?? []
        offer = try? c.decodeIfPresent(SkillOffer.self, forKey: .offer)
    }
}

public struct SkillSummary: Codable, Hashable, Identifiable, Sendable {
    public let name: String
    public let description: String
    public let path: String
    /// `user`, `workspace`, `env` or `other`.
    public let scope: String
    public var id: String { path }
}

/// Which skills the current model is offered, and the rule's own words.
public struct SkillOffer: Codable, Hashable, Sendable {
    public let model: String?
    public let tier: String
    public let reason: String
    public let withheld: [String]

    enum CodingKeys: String, CodingKey { case model, tier, reason, withheld }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        model = try? c.decodeIfPresent(String.self, forKey: .model)
        tier = (try? c.decode(String.self, forKey: .tier)) ?? "full"
        reason = (try? c.decode(String.self, forKey: .reason)) ?? ""
        withheld = (try? c.decodeIfPresent([String].self, forKey: .withheld)) ?? []
    }
}

/// `GET /api/sessions/:id/skills/active`.
public struct ActiveSkillListing: Codable, Sendable {
    public let skills: [ActiveSkill]
    public let loaded: Int
    public let active: Int
}

public struct ActiveSkill: Codable, Hashable, Identifiable, Sendable {
    public let name: String
    /// `present`, `elided`, `dropped` or `unrecorded`.
    public let presence: String
    public let active: Bool
    public let loads: Int
    public var id: String { name }
}

/// `GET /api/agents`.
public struct AgentListing: Codable, Sendable {
    public let agents: [AgentSummary]
    public let defaultName: String

    enum CodingKeys: String, CodingKey {
        case agents
        case defaultName = "default"
    }
}

public struct AgentSummary: Codable, Hashable, Identifiable, Sendable {
    public let name: String
    /// `user` or `workspace`.
    public let scope: String
    public let description: String?
    public let model: String?
    public let wantsPrivilege: Bool
    public let privilegeSummary: [String]
    public let trusted: Bool
    public let fullyApplied: Bool
    public let broken: String?

    public var id: String { "\(scope)/\(name)" }

    enum CodingKeys: String, CodingKey {
        case name, scope, description, model, trusted, broken
        case wantsPrivilege = "wants_privilege"
        case privilegeSummary = "privilege_summary"
        case fullyApplied = "fully_applied"
    }
}

/// `GET /api/voice/status`, and `voice` inside `/api/capabilities`.
public struct VoiceStatus: Codable, Hashable, Sendable {
    /// The server was built with voice. It says nothing about whether the
    /// speech runtime is running: that is `ready`.
    public let available: Bool
    public let runtimeReachable: Bool
    public let modelPresent: Bool
    public let setupAvailable: Bool
    public let stage: String?
    public let detail: String

    enum CodingKeys: String, CodingKey {
        case available, stage, detail
        case runtimeReachable = "runtime_reachable"
        case modelPresent = "model_present"
        case setupAvailable = "setup_available"
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        available = (try? c.decode(Bool.self, forKey: .available)) ?? false
        runtimeReachable = (try? c.decode(Bool.self, forKey: .runtimeReachable)) ?? false
        modelPresent = (try? c.decode(Bool.self, forKey: .modelPresent)) ?? false
        setupAvailable = (try? c.decode(Bool.self, forKey: .setupAvailable)) ?? false
        stage = try? c.decodeIfPresent(String.self, forKey: .stage)
        detail = (try? c.decode(String.self, forKey: .detail)) ?? "Voice input is unavailable."
    }

    /// The runtime answered and has its model, so a recording can be
    /// transcribed now. Anything less needs the readiness request first.
    public var ready: Bool { available && runtimeReachable && modelPresent }
}

/// One frame of `POST /api/voice/wait`.
public struct VoiceWaitEvent: Codable, Hashable, Sendable {
    /// `waiting`, `ready` or `error`.
    public let status: String
    public let stage: String
    public let detail: String
}

/// One reviewing angle a panel can be built from. Code-defined on the
/// server; this app only chooses among them.
public struct PanelLens: Codable, Hashable, Identifiable, Sendable {
    public let name: String
    public let instruction: String
    public var id: String { name }
}

struct LensList: Codable {
    let lenses: [PanelLens]
}

/// `GET /api/investigate/status`.
public struct InvestigateStatus: Codable, Sendable {
    public let available: Bool
    public let forecasting: Bool
}

/// The three numbers a first attempt on a question commits to.
public struct Preregistration: Sendable, Equatable {
    public var metricName: String
    public var killThreshold: Double
    /// `lower-is-better` or `higher-is-better`.
    public var direction: String

    public init(metricName: String, killThreshold: Double, direction: String) {
        self.metricName = metricName
        self.killThreshold = killThreshold
        self.direction = direction
    }
}

/// `GET /api/recall/status`.
public struct RecallStatus: Codable, Sendable {
    public let available: Bool
    public let reason: String?
    public let conversations: Int
    public let chunks: Int
    public let memory: Bool

    enum CodingKeys: String, CodingKey { case available, reason, conversations, chunks, memory }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        available = (try? c.decode(Bool.self, forKey: .available)) ?? false
        reason = try? c.decodeIfPresent(String.self, forKey: .reason)
        conversations = (try? c.decode(Int.self, forKey: .conversations)) ?? 0
        chunks = (try? c.decode(Int.self, forKey: .chunks)) ?? 0
        memory = (try? c.decode(Bool.self, forKey: .memory)) ?? false
    }
}

/// One hit from `GET /api/recall/search`.
public struct RecallHit: Codable, Hashable, Identifiable, Sendable {
    public let id: String
    public let title: String
    public let seq: Int64
    public let role: String
    public let snippet: String
    public let score: Double
}

struct RecallHits: Codable {
    let hits: [RecallHit]?
}

/// `GET /api/mcp`.
public struct McpListing: Codable, Sendable {
    public let servers: [McpServer]
    public let loadsServers: Bool
    public let why: String
    public let sources: [String]
    public let warning: String?

    enum CodingKeys: String, CodingKey {
        case servers, why, sources, warning
        case loadsServers = "loads_servers"
    }
}

public struct McpServer: Codable, Hashable, Identifiable, Sendable {
    public let name: String
    public let transport: String
    public let command: [String]?
    public let url: String?
    public let trust: String
    public let loaded: Bool
    public var id: String { name }
}

/// `GET /api/data`.
public struct DataState: Codable, Sendable {
    public let files: [DataFile]
    public let resetNote: String

    enum CodingKeys: String, CodingKey {
        case files
        case resetNote = "reset_note"
    }
}

public struct DataFile: Codable, Hashable, Identifiable, Sendable {
    public let label: String
    public let what: String
    public let path: String
    public let envVar: String
    public let exists: Bool
    public let bytes: Int64?
    public var id: String { path }

    enum CodingKeys: String, CodingKey {
        case label, what, path, exists, bytes
        case envVar = "env_var"
    }
}

/// `GET /api/doctor`.
public struct DoctorReport: Codable, Sendable {
    public let version: String
    public let healthy: Bool
    public let baseURL: String
    public let checks: [DoctorCheck]

    enum CodingKeys: String, CodingKey {
        case version, healthy, checks
        case baseURL = "base_url"
    }
}

public struct DoctorCheck: Codable, Hashable, Sendable {
    /// `ok`, `bad`, `off` or `note`.
    public let health: String
    public let label: String
    public let detail: String
}
