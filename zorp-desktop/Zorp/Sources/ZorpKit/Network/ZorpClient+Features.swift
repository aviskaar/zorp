import Foundation

/// The routes behind the optional features. Each exists in every server
/// build; one whose feature was compiled out answers 501 with a sentence
/// saying so, which the caller shows rather than guessing.
extension ZorpClient {
    // MARK: skills

    public func skills() async throws -> SkillListing {
        try await json(SkillListing.self, "GET", "api/skills")
    }

    public func activeSkills(id: String) async throws -> ActiveSkillListing {
        try await json(ActiveSkillListing.self, "GET", "api/sessions/\(id)/skills/active")
    }

    // MARK: agents

    public func agents() async throws -> AgentListing {
        try await json(AgentListing.self, "GET", "api/agents")
    }

    public func trustAgent(scope: String, name: String) async throws {
        try await send("POST", "api/agents/\(scope)/\(name)/trust", body: encode([String: Any]()))
    }

    /// Pick the agent a conversation runs under. Locked once it answered.
    public func setSessionAgent(id: String, agent: String?) async throws {
        try await send("PUT", "api/sessions/\(id)/agent", body: encode(["agent": orNull(agent)]))
    }

    // MARK: voice

    public func voiceStatus() async throws -> VoiceStatus {
        try await json(VoiceStatus.self, "GET", "api/voice/status")
    }

    /// Ask the server to get the voice model ready, reporting each stage.
    /// Setup can take minutes the first time: it creates an environment,
    /// installs the runtime and downloads the model.
    public func waitForVoice(onEvent: @escaping @Sendable (VoiceWaitEvent) -> Void) async throws {
        var request = URLRequest(url: url("api/voice/wait"))
        request.httpMethod = "POST"
        // The server refuses this without a JSON content type, so that a
        // cross-origin page cannot start setup with a simple request.
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.httpBody = Data("{}".utf8)
        request.timeoutInterval = 3600
        let (bytes, response): (URLSession.AsyncBytes, URLResponse)
        do {
            (bytes, response) = try await session.bytes(for: request)
        } catch {
            throw APIError.transport(error.localizedDescription)
        }
        guard let http = response as? HTTPURLResponse else { throw APIError.transport("no HTTP response") }
        guard (200...299).contains(http.statusCode) else {
            var text = ""
            for try await line in bytes.lines { text += line + "\n" }
            throw APIError.http(status: http.statusCode, body: text)
        }
        for try await line in bytes.lines {
            guard let payload = SSEStream.dataField(line),
                  let event = try? JSONDecoder().decode(VoiceWaitEvent.self, from: Data(payload.utf8)) else { continue }
            onEvent(event)
            if event.status == "error" { throw APIError.http(status: 503, body: event.detail) }
            if event.status == "ready" { return }
        }
        // Ending without either is not ready: recording now would send the
        // audio to a runtime that never came up.
        throw APIError.transport("the voice setup stream ended before the model was ready")
    }

    // MARK: panel

    public func lenses() async throws -> [PanelLens] {
        try await json(LensList.self, "GET", "api/panel/lenses").lenses
    }

    /// Launch a review panel on `body`. Its reviewers report on the
    /// session's event stream.
    public func startPanel(id: String, label: String, body: String, lenses: [String]) async throws {
        try await send("POST", "api/sessions/\(id)/panel", body: encode(["label": label, "body": body, "lenses": lenses]))
    }

    // MARK: investigate

    public func investigateStatus() async throws -> InvestigateStatus {
        try await json(InvestigateStatus.self, "GET", "api/investigate/status")
    }

    public func startInvestigate(id: String, question: String, prereg: Preregistration?, askAtCheckpoints: Bool) async throws {
        let body: [String: Any] = [
            "question": question,
            "metric_name": orNull(prereg?.metricName),
            "kill_threshold": orNull(prereg?.killThreshold),
            "threshold_direction": orNull(prereg?.direction),
            "interactive_checkpoints": askAtCheckpoints,
        ]
        try await send("POST", "api/sessions/\(id)/investigate", body: encode(body))
    }

    /// Let the running attempt finish and be recorded, then write up.
    public func stopAfterAttempt(id: String) async throws {
        try await send("POST", "api/sessions/\(id)/investigate/stop-after")
    }

    public func ledger(question: String) async throws -> LedgerFrame {
        try await json(LedgerFrame.self, "GET", "api/investigate/ledger", query: [URLQueryItem(name: "question", value: question)])
    }

    // MARK: recall

    public func recallStatus() async throws -> RecallStatus {
        try await json(RecallStatus.self, "GET", "api/recall/status")
    }

    public func recallSearch(query: String, project: String? = nil, limit: Int = 20) async throws -> [RecallHit] {
        var items = [URLQueryItem(name: "q", value: query), URLQueryItem(name: "limit", value: String(limit))]
        if let project { items.append(URLQueryItem(name: "project", value: project)) }
        return try await json(RecallHits.self, "GET", "api/recall/search", query: items).hits ?? []
    }

    // MARK: settings panes

    public func mcp() async throws -> McpListing {
        try await json(McpListing.self, "GET", "api/mcp")
    }

    public func dataState() async throws -> DataState {
        try await json(DataState.self, "GET", "api/data")
    }

    public func clearAllSessions() async throws {
        try await send("DELETE", "api/sessions")
    }

    public func deleteRecallIndex() async throws {
        try await send("DELETE", "api/recall/index")
    }

    public func resetSettings() async throws {
        try await send("DELETE", "api/settings")
    }

    public func doctor(probe: Bool) async throws -> DoctorReport {
        try await json(DoctorReport.self, "GET", "api/doctor", query: probe ? [URLQueryItem(name: "probe", value: "1")] : [])
    }
}

/// A JSON `null` for a missing value, so a body says "none" rather than
/// leaving the key out.
func orNull<T>(_ value: T?) -> Any {
    if let value { return value }
    return NSNull()
}
