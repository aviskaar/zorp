import Foundation

/// What a request to the in-process server can fail with.
///
/// The server answers a refusal with a status and one plain sentence, and
/// that sentence is what a person should see. Collapsing it into
/// `URLError(.badServerResponse)` is how sending a message used to fail
/// with nothing but an NSURL error code.
public enum APIError: LocalizedError, Equatable, Sendable {
    case http(status: Int, body: String)
    case transport(String)
    case decoding(String)

    /// The exact sentence `zorp-web` answers a turn with when no workspace
    /// has been chosen. It is matched on, the way the browser matches it.
    public static let noWorkspace = "no workspace chosen"

    public var errorDescription: String? {
        switch self {
        case .http(let status, let body):
            let text = body.trimmingCharacters(in: .whitespacesAndNewlines)
            return text.isEmpty ? "The server answered \(status)." : text
        case .transport(let message):
            return "Could not reach the Zorp server: \(message)"
        case .decoding(let message):
            return "The server sent something this app could not read: \(message)"
        }
    }

    public var status: Int? {
        if case .http(let status, _) = self { return status }
        return nil
    }

    public var isNoWorkspace: Bool {
        if case .http(409, let body) = self {
            return body.trimmingCharacters(in: .whitespacesAndNewlines) == APIError.noWorkspace
        }
        return false
    }
}

/// A client for `zorp-web`'s HTTP API, one method per route this app uses.
/// It mirrors `web/src/api.ts`.
public final class ZorpClient: Sendable {
    public let baseURL: URL
    private let session: URLSession

    public init(baseURL: URL, session: URLSession = .shared) {
        self.baseURL = baseURL
        self.session = session
    }

    // MARK: plumbing

    func url(_ path: String, query: [URLQueryItem] = []) -> URL {
        let encoded = "/" + path.split(separator: "/").map { Self.segment(String($0)) }.joined(separator: "/")
        return url(encodedPath: encoded, query: query)
    }

    /// A URL whose path is already percent-encoded, for a segment that
    /// must carry a slash, such as a checkpoint directory.
    func url(encodedPath: String, query: [URLQueryItem] = []) -> URL {
        var components = URLComponents(url: baseURL, resolvingAgainstBaseURL: false)!
        components.percentEncodedPath = encodedPath
        components.queryItems = query.isEmpty ? nil : query
        return components.url!
    }

    /// One path segment, with everything but unreserved characters encoded,
    /// a slash included. `encodeURIComponent` does the same in the browser.
    static func segment(_ raw: String) -> String {
        var allowed = CharacterSet.alphanumerics
        allowed.insert(charactersIn: "-._~")
        return raw.addingPercentEncoding(withAllowedCharacters: allowed) ?? raw
    }

    @discardableResult
    func send(
        _ method: String, _ path: String, query: [URLQueryItem] = [],
        body: Data? = nil, contentType: String = "application/json",
        encodedPath: String? = nil
    ) async throws -> Data {
        var request = URLRequest(url: encodedPath.map { url(encodedPath: $0, query: query) } ?? url(path, query: query))
        request.httpMethod = method
        if let body {
            request.httpBody = body
            request.setValue(contentType, forHTTPHeaderField: "Content-Type")
        }
        let data: Data
        let response: URLResponse
        do {
            (data, response) = try await session.data(for: request)
        } catch {
            throw APIError.transport(error.localizedDescription)
        }
        guard let http = response as? HTTPURLResponse else {
            throw APIError.transport("no HTTP response")
        }
        guard (200...299).contains(http.statusCode) else {
            throw APIError.http(status: http.statusCode, body: String(decoding: data, as: UTF8.self))
        }
        return data
    }

    func json<T: Decodable>(
        _ type: T.Type, _ method: String, _ path: String,
        query: [URLQueryItem] = [], body: Data? = nil
    ) async throws -> T {
        let data = try await send(method, path, query: query, body: body)
        do {
            return try JSONDecoder().decode(T.self, from: data)
        } catch {
            throw APIError.decoding("\(path): \(error)")
        }
    }

    func encode(_ object: [String: Any]) throws -> Data {
        try JSONSerialization.data(withJSONObject: object)
    }

    func encodeValue<T: Encodable>(_ value: T) throws -> Data {
        try JSONEncoder().encode(value)
    }

    // MARK: health

    public func isReachable() async -> Bool {
        (try? await send("GET", "api/health")) != nil
    }

    // MARK: sessions

    public func listSessions() async throws -> [SessionSummary] {
        try await json([SessionSummary].self, "GET", "api/sessions")
    }

    public func createSession() async throws -> String {
        try await json(IDResponse.self, "POST", "api/sessions").id
    }

    public func getSession(id: String) async throws -> Transcript {
        try await json(Transcript.self, "GET", "api/sessions/\(id)")
    }

    public func deleteSession(id: String) async throws {
        try await send("DELETE", "api/sessions/\(id)")
    }

    /// Copy a conversation up to and including its `answer`th answer,
    /// counted from 1, into a new session, and return the new id.
    public func branchSession(id: String, answer: Int) async throws -> String {
        try await json(IDResponse.self, "POST", "api/sessions/\(id)/branch", body: encode(["answer": answer])).id
    }

    public func compactSession(id: String, focus: String? = nil) async throws -> CompactResult {
        var body: [String: Any] = [:]
        if let focus, !focus.isEmpty { body["focus"] = focus }
        return try await json(CompactResult.self, "POST", "api/sessions/\(id)/compact", body: encode(body))
    }

    /// Start a turn. The answer arrives on the event stream, not here.
    public func sendTurn(id: String, message: String, memory: Bool = false) async throws {
        try await send("POST", "api/sessions/\(id)/turn", body: encode(["message": message, "memory": memory]))
    }

    /// Ask the running turn to stop. A 409 means nothing was running.
    public func stopTurn(id: String) async throws {
        try await send("POST", "api/sessions/\(id)/stop")
    }

    public func approve(id: String, allow: Bool) async throws {
        try await send("POST", "api/sessions/\(id)/approve", body: encode(["allow": allow]))
    }

    public func resolveCheckpoint(id: String, allow: Bool) async throws {
        try await send("POST", "api/sessions/\(id)/checkpoint", body: encode(["allow": allow]))
    }

    public func getAutoApprove(id: String) async throws -> Bool {
        try await json(AutoApproveResponse.self, "GET", "api/sessions/\(id)/auto-approve").autoApprove
    }

    @discardableResult
    public func setAutoApprove(id: String, on: Bool) async throws -> Bool {
        try await json(AutoApproveResponse.self, "POST", "api/sessions/\(id)/auto-approve", body: encode(["on": on])).autoApprove
    }

    public func eventsURL(id: String) -> URL {
        url("api/sessions/\(id)/events")
    }

    // MARK: projects

    public func listProjects() async throws -> [Project] {
        try await json(ProjectList.self, "GET", "api/projects").projects
    }

    public func createProject(name: String) async throws -> Project {
        try await json(Project.self, "POST", "api/projects", body: encode(["name": name]))
    }

    public func deleteProject(id: String) async throws {
        try await send("DELETE", "api/projects/\(id)")
    }

    public func setSessionProject(id: String, projectId: String?) async throws {
        let body: [String: Any] = ["project_id": projectId ?? NSNull()]
        try await send("PUT", "api/sessions/\(id)/project", body: encode(body))
    }

    // MARK: workspace

    public func getWorkspace() async throws -> Workspace {
        try await json(Workspace.self, "GET", "api/workspace")
    }

    /// Choose the directory the agent works in. A 400 carries the reason
    /// the server refused the path.
    public func setWorkspace(path: String) async throws -> Workspace {
        try await json(Workspace.self, "PUT", "api/workspace", body: encode(["path": path]))
    }

    // MARK: settings

    public func getSettings() async throws -> Settings {
        try await json(Settings.self, "GET", "api/settings")
    }

    public func putSettings(_ update: SettingsUpdate) async throws -> Settings {
        try await json(Settings.self, "PUT", "api/settings", body: encodeValue(update))
    }

    /// List the models an endpoint offers. A failure is in `error`, not
    /// thrown: an unreachable endpoint is an answer, not a server error.
    public func listModels(baseURL: String, apiKey: String? = nil) async throws -> ModelsList {
        var body: [String: Any] = ["base_url": baseURL]
        if let apiKey, !apiKey.isEmpty { body["api_key"] = apiKey }
        return try await json(ModelsList.self, "POST", "api/settings/models", body: encode(body))
    }

    public func testConnection(provider: ModelProvider, baseURL: String, model: String, apiKey: String? = nil) async throws -> ConnectionTest {
        var body: [String: Any] = ["provider": provider.rawValue, "base_url": baseURL, "model": model]
        if let apiKey, !apiKey.isEmpty { body["api_key"] = apiKey }
        return try await json(ConnectionTest.self, "POST", "api/settings/test", body: encode(body))
    }

    public func capabilities() async throws -> Capabilities {
        try await json(Capabilities.self, "GET", "api/capabilities")
    }

    // MARK: files

    public func listArtifacts() async throws -> ArtifactListing {
        try await json(ArtifactListing.self, "GET", "api/artifacts")
    }

    public func readArtifact(path: String) async throws -> Data {
        try await send("GET", "api/artifacts/raw", query: [URLQueryItem(name: "path", value: path)])
    }

    // MARK: voice

    public func transcribe(audio: Data, mimeType: String) async throws -> VoiceTranscription {
        let data = try await send("POST", "api/voice/transcribe", body: audio, contentType: mimeType)
        do {
            return try JSONDecoder().decode(VoiceTranscription.self, from: data)
        } catch {
            throw APIError.decoding("api/voice/transcribe: \(error)")
        }
    }
}
