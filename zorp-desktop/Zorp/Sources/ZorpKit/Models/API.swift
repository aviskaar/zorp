import Foundation

// The shapes zorp-web answers with, field for field. Each type names the
// route it comes from. They mirror `web/src/api.ts`, which is the other
// client of the same server, so when one of them changes the other should
// change with it.

/// One row of `GET /api/sessions`.
public struct SessionSummary: Codable, Identifiable, Hashable, Sendable {
    public let id: String
    public let title: String
    /// `running`, `done`, `stopped` or `error` for a stored row; an
    /// in-memory session that has not recorded a message reads `running`.
    public let status: String?
    public let projectId: String?
    public let agent: String?
    /// Milliseconds since the epoch. Only a stored session has one, which
    /// is how a conversation that can be filed is told from one that exists
    /// only in the server's memory.
    public let updated: Int64?

    enum CodingKeys: String, CodingKey {
        case id, title, status, agent, updated
        case projectId = "project_id"
    }

    public init(
        id: String, title: String, status: String? = nil, projectId: String? = nil,
        agent: String? = nil, updated: Int64? = nil
    ) {
        self.id = id
        self.title = title
        self.status = status
        self.projectId = projectId
        self.agent = agent
        self.updated = updated
    }

    public var isStored: Bool { updated != nil }
}

/// One project, from `GET /api/projects` (inside `projects`) or
/// `POST /api/projects`.
public struct Project: Codable, Identifiable, Hashable, Sendable {
    public let id: String
    public let name: String
    public let created: Int64?

    public init(id: String, name: String, created: Int64? = nil) {
        self.id = id
        self.name = name
        self.created = created
    }
}

struct ProjectList: Codable {
    let projects: [Project]
}

/// One entry of a replayed transcript from `GET /api/sessions/:id`. The
/// server sends `{role, content}` for a message and
/// `{role: "tool", name, summary, phrase?}` for a stored tool call.
public struct TranscriptEntry: Codable, Hashable, Sendable {
    public let role: String
    public let content: String?
    public let name: String?
    public let summary: String?
    public let phrase: String?

    public init(role: String, content: String? = nil, name: String? = nil, summary: String? = nil, phrase: String? = nil) {
        self.role = role
        self.content = content
        self.name = name
        self.summary = summary
        self.phrase = phrase
    }
}

/// A compaction marker on a replayed transcript. `after` is how many
/// entries it follows, counted by the server.
public struct CompactionRecord: Codable, Hashable, Sendable {
    public let id: Int64
    public let after: Int
    public let messages: Int
    public let summary: String
    public let tokensBefore: Int64
    public let tokensAfter: Int64
    public let manual: Bool

    enum CodingKeys: String, CodingKey {
        case id, after, messages, summary, manual
        case tokensBefore = "tokens_before"
        case tokensAfter = "tokens_after"
    }
}

public struct Transcript: Codable, Sendable {
    public let messages: [TranscriptEntry]
    public let compactions: [CompactionRecord]
    public let agent: String?

    public init(messages: [TranscriptEntry], compactions: [CompactionRecord] = [], agent: String? = nil) {
        self.messages = messages
        self.compactions = compactions
        self.agent = agent
    }

    enum CodingKeys: String, CodingKey {
        case messages, compactions, agent
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        messages = try c.decode([TranscriptEntry].self, forKey: .messages)
        compactions = try c.decodeIfPresent([CompactionRecord].self, forKey: .compactions) ?? []
        agent = try c.decodeIfPresent(String.self, forKey: .agent)
    }
}

/// `POST /api/sessions/:id/compact`.
public struct CompactResult: Codable, Sendable {
    public let compacted: Bool
    public let reason: String?
    public let tokensBefore: Int64?
    public let tokensAfter: Int64?

    enum CodingKeys: String, CodingKey {
        case compacted, reason
        case tokensBefore = "tokens_before"
        case tokensAfter = "tokens_after"
    }
}

/// `GET` and `PUT /api/workspace`.
public struct Workspace: Codable, Hashable, Sendable {
    public let path: String?
    public let scratch: String?
    /// `flag`, `env`, `saved` or `none`.
    public let source: String
    public let configured: Bool

    public init(path: String?, scratch: String? = nil, source: String = "none", configured: Bool) {
        self.path = path
        self.scratch = scratch
        self.source = source
        self.configured = configured
    }
}

/// `openai` or `anthropic`, the two wire formats the agent speaks.
public enum ModelProvider: String, Codable, CaseIterable, Identifiable, Sendable {
    case openai
    case anthropic

    public var id: String { rawValue }

    public var label: String {
        switch self {
        case .openai: return "OpenAI-compatible"
        case .anthropic: return "Anthropic"
        }
    }
}

/// Where a setting came from: the settings file or this app (`ui`), an
/// environment variable (`env`), or the compiled-in default.
public enum SettingsSource: String, Codable, Sendable {
    case ui, env, `default`
}

/// `GET /api/settings`. The API key itself is never in it, only whether
/// one is set.
public struct Settings: Codable, Hashable, Sendable {
    public let provider: ModelProvider
    public let providerSource: SettingsSource
    public let baseURL: String
    public let baseURLSource: SettingsSource
    public let model: String
    public let modelSource: SettingsSource
    public let maxTokens: Int?
    public let maxTokensSource: SettingsSource
    public let hasAPIKey: Bool
    public let apiKeySource: SettingsSource
    public let configured: Bool

    enum CodingKeys: String, CodingKey {
        case provider, model, configured
        case providerSource = "provider_source"
        case baseURL = "base_url"
        case baseURLSource = "base_url_source"
        case modelSource = "model_source"
        case maxTokens = "max_tokens"
        case maxTokensSource = "max_tokens_source"
        case hasAPIKey = "has_api_key"
        case apiKeySource = "api_key_source"
    }
}

/// `PUT /api/settings`. Every field is optional and a missing one is left
/// as it is.
public struct SettingsUpdate: Encodable, Sendable {
    public var provider: ModelProvider?
    public var baseURL: String?
    public var model: String?
    public var maxTokens: Int?
    public var apiKey: String?

    public init(provider: ModelProvider? = nil, baseURL: String? = nil, model: String? = nil, maxTokens: Int? = nil, apiKey: String? = nil) {
        self.provider = provider
        self.baseURL = baseURL
        self.model = model
        self.maxTokens = maxTokens
        self.apiKey = apiKey
    }

    enum CodingKeys: String, CodingKey {
        case provider, model
        case baseURL = "base_url"
        case maxTokens = "max_tokens"
        case apiKey = "api_key"
    }

    public func encode(to encoder: Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        try c.encodeIfPresent(provider, forKey: .provider)
        try c.encodeIfPresent(baseURL, forKey: .baseURL)
        try c.encodeIfPresent(model, forKey: .model)
        try c.encodeIfPresent(maxTokens, forKey: .maxTokens)
        try c.encodeIfPresent(apiKey, forKey: .apiKey)
    }
}

/// One model from `/api/settings/models`, with whatever the endpoint said
/// about it beyond its id.
public struct ModelDetail: Codable, Hashable, Identifiable, Sendable {
    public let id: String
    public let name: String?
    public let contextLength: Int?
    public let promptPrice: Double?
    public let completionPrice: Double?
    public let outputModalities: [String]?

    enum CodingKeys: String, CodingKey {
        case id, name
        case contextLength = "context_length"
        case promptPrice = "prompt_price"
        case completionPrice = "completion_price"
        case outputModalities = "output_modalities"
    }

    public init(id: String, name: String? = nil, contextLength: Int? = nil, promptPrice: Double? = nil, completionPrice: Double? = nil, outputModalities: [String]? = nil) {
        self.id = id
        self.name = name
        self.contextLength = contextLength
        self.promptPrice = promptPrice
        self.completionPrice = completionPrice
        self.outputModalities = outputModalities
    }

    /// Free only when the listing stated a price of zero for both sides.
    /// A model with no stated price is not free, it is unpriced.
    public var isFree: Bool {
        promptPrice == 0 && completionPrice == 0
    }

    /// False only when the listing said the model answers with something
    /// other than text. Saying nothing is not saying "not text".
    public var answersWithText: Bool {
        guard let modes = outputModalities else { return true }
        return modes.contains("text")
    }
}

/// `POST /api/settings/models`.
public struct ModelsList: Codable, Sendable {
    public let models: [String]
    public let details: [ModelDetail]
    public let error: String?

    enum CodingKeys: String, CodingKey {
        case models, details, error
    }

    public init(models: [String], details: [ModelDetail] = [], error: String? = nil) {
        self.models = models
        self.details = details
        self.error = error
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        models = try c.decodeIfPresent([String].self, forKey: .models) ?? []
        details = try c.decodeIfPresent([ModelDetail].self, forKey: .details) ?? []
        error = try c.decodeIfPresent(String.self, forKey: .error)
    }

    /// The details when the server sent them, and bare ids otherwise.
    public var entries: [ModelDetail] {
        details.isEmpty ? models.map { ModelDetail(id: $0) } : details
    }
}

/// `POST /api/settings/test`.
public struct ConnectionTest: Codable, Sendable {
    public let ok: Bool
    public let reason: String?
}

public struct ToolAvailability: Codable, Hashable, Sendable {
    public let available: Bool
    public let detail: String
}

/// `GET /api/capabilities`, the parts this app reads.
public struct Capabilities: Codable, Sendable {
    public let webSearch: ToolAvailability?

    enum CodingKeys: String, CodingKey {
        case webSearch = "web_search"
    }
}

/// One file the Files pane can open, from `GET /api/artifacts`.
public struct Artifact: Codable, Hashable, Identifiable, Sendable {
    public let path: String
    public let bytes: Int64
    public let modifiedMs: Int64

    public var id: String { path }

    enum CodingKeys: String, CodingKey {
        case path, bytes
        case modifiedMs = "modified_ms"
    }

    public init(path: String, bytes: Int64 = 0, modifiedMs: Int64 = 0) {
        self.path = path
        self.bytes = bytes
        self.modifiedMs = modifiedMs
    }
}

public struct ArtifactListing: Codable, Sendable {
    public let files: [Artifact]
    public let truncated: Bool
}

/// `POST /api/voice/transcribe`.
public struct VoiceTranscription: Codable, Sendable {
    public let text: String
    public let language: String?
}

struct IDResponse: Codable {
    let id: String
}

struct AutoApproveResponse: Codable {
    let autoApprove: Bool

    enum CodingKeys: String, CodingKey {
        case autoApprove = "auto_approve"
    }
}
