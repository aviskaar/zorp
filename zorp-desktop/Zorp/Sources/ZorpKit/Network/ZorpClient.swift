import Foundation

public actor ZorpClient {
    public let baseURL: URL
    private let session: URLSession

    public init(baseURL: URL, session: URLSession = .shared) {
        self.baseURL = baseURL
        self.session = session
    }

    public func listSessions() async throws -> [SessionSummary] {
        let url = baseURL.appendingPathComponent("api/sessions")
        let (data, response) = try await session.data(from: url)
        guard let httpResponse = response as? HTTPURLResponse, (200...299).contains(httpResponse.statusCode) else {
            throw URLError(.badServerResponse)
        }
        guard let list = try? JSONDecoder().decode([SessionSummary].self, from: data) else {
            // Might be wrapped in an array or dictionary
            if let arr = try? JSONSerialization.jsonObject(with: data) as? [[String: Any]] {
                let serialized = try JSONSerialization.data(withJSONObject: arr)
                return (try? JSONDecoder().decode([SessionSummary].self, from: serialized)) ?? []
            }
            return []
        }
        return list
    }

    public func createSession() async throws -> String {
        let url = baseURL.appendingPathComponent("api/sessions")
        var request = URLRequest(url: url)
        request.httpMethod = "POST"

        let (data, response) = try await session.data(for: request)
        guard let httpResponse = response as? HTTPURLResponse, (200...299).contains(httpResponse.statusCode),
              let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let id = json["id"] as? String else {
            throw URLError(.badServerResponse)
        }
        return id
    }

    public func deleteSession(id: String) async throws {
        let url = baseURL.appendingPathComponent("api/sessions/\(id)")
        var request = URLRequest(url: url)
        request.httpMethod = "DELETE"
        _ = try await session.data(for: request)
    }

    public func startTurn(sessionId: String, prompt: String) async throws {
        let url = baseURL.appendingPathComponent("api/sessions/\(sessionId)/turn")
        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        let body: [String: Any] = ["prompt": prompt]
        request.httpBody = try JSONSerialization.data(withJSONObject: body)

        let (_, response) = try await session.data(for: request)
        guard let httpResponse = response as? HTTPURLResponse, (200...299).contains(httpResponse.statusCode) else {
            throw URLError(.badServerResponse)
        }
    }

    public func stopTurn(sessionId: String) async throws {
        let url = baseURL.appendingPathComponent("api/sessions/\(sessionId)/stop")
        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        _ = try await session.data(for: request)
    }

    public func submitApproval(sessionId: String, requestId: String, approved: Bool) async throws {
        let url = baseURL.appendingPathComponent("api/sessions/\(sessionId)/approve")
        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        let body: [String: Any] = ["id": requestId, "approved": approved]
        request.httpBody = try JSONSerialization.data(withJSONObject: body)
        _ = try await session.data(for: request)
    }

    public func submitCheckpoint(sessionId: String, checkpointId: String, approved: Bool) async throws {
        let url = baseURL.appendingPathComponent("api/sessions/\(sessionId)/checkpoint")
        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        let body: [String: Any] = ["id": checkpointId, "approved": approved]
        request.httpBody = try JSONSerialization.data(withJSONObject: body)
        _ = try await session.data(for: request)
    }

    public func startInvestigate(sessionId: String, attempts: Int = 3) async throws {
        let url = baseURL.appendingPathComponent("api/sessions/\(sessionId)/investigate")
        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        let body: [String: Any] = ["attempts": attempts]
        request.httpBody = try JSONSerialization.data(withJSONObject: body)
        _ = try await session.data(for: request)
    }

    public func stopAfterAttempt(sessionId: String) async throws {
        let url = baseURL.appendingPathComponent("api/sessions/\(sessionId)/investigate/stop-after")
        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        _ = try await session.data(for: request)
    }

    public func listArtifacts() async throws -> [ArtifactItem] {
        let url = baseURL.appendingPathComponent("api/artifacts")
        let (data, response) = try await session.data(from: url)
        guard let httpResponse = response as? HTTPURLResponse, (200...299).contains(httpResponse.statusCode) else {
            return []
        }
        return (try? JSONDecoder().decode([ArtifactItem].self, from: data)) ?? []
    }

    public func readArtifact(path: String) async throws -> String {
        var components = URLComponents(url: baseURL.appendingPathComponent("api/artifacts/raw"), resolvingAgainstBaseURL: true)!
        components.queryItems = [URLQueryItem(name: "path", value: path)]
        let (data, response) = try await session.data(from: components.url!)
        guard let httpResponse = response as? HTTPURLResponse, (200...299).contains(httpResponse.statusCode) else {
            throw URLError(.badServerResponse)
        }
        return String(data: data, encoding: .utf8) ?? ""
    }

    public func listProjects() async throws -> [ProjectItem] {
        let url = baseURL.appendingPathComponent("api/projects")
        let (data, response) = try await session.data(from: url)
        guard let httpResponse = response as? HTTPURLResponse, (200...299).contains(httpResponse.statusCode) else {
            return []
        }
        return (try? JSONDecoder().decode([ProjectItem].self, from: data)) ?? []
    }

    public func createProject(name: String) async throws -> ProjectItem {
        let url = baseURL.appendingPathComponent("api/projects")
        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        let body: [String: Any] = ["name": name]
        request.httpBody = try JSONSerialization.data(withJSONObject: body)
        let (data, response) = try await session.data(for: request)
        guard let httpResponse = response as? HTTPURLResponse, (200...299).contains(httpResponse.statusCode) else {
            throw URLError(.badServerResponse)
        }
        return try JSONDecoder().decode(ProjectItem.self, from: data)
    }

    public func setSessionProject(sessionId: String, projectId: String?) async throws {
        let url = baseURL.appendingPathComponent("api/sessions/\(sessionId)/project")
        var request = URLRequest(url: url)
        request.httpMethod = "PUT"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        let body: [String: Any] = ["project_id": projectId as Any]
        request.httpBody = try JSONSerialization.data(withJSONObject: body)
        _ = try await session.data(for: request)
    }

    public func transcribeVoice(audioData: Data, mimeType: String = "audio/wav") async throws -> String {
        let url = baseURL.appendingPathComponent("api/voice/transcribe")
        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        request.setValue(mimeType, forHTTPHeaderField: "Content-Type")
        request.httpBody = audioData

        let (data, response) = try await session.data(for: request)
        guard let httpResponse = response as? HTTPURLResponse, (200...299).contains(httpResponse.statusCode),
              let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let text = json["text"] as? String else {
            throw URLError(.badServerResponse)
        }
        return text
    }

    public func setAutoApprove(sessionId: String, autoApprove: Bool) async throws {
        let url = baseURL.appendingPathComponent("api/sessions/\(sessionId)/auto-approve")
        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        let body: [String: Any] = ["enabled": autoApprove]
        request.httpBody = try JSONSerialization.data(withJSONObject: body)
        _ = try await session.data(for: request)
    }

    public func getAutoApprove(sessionId: String) async throws -> Bool {
        let url = baseURL.appendingPathComponent("api/sessions/\(sessionId)/auto-approve")
        let (data, response) = try await session.data(from: url)
        guard let httpResponse = response as? HTTPURLResponse, (200...299).contains(httpResponse.statusCode),
              let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let enabled = json["enabled"] as? Bool else {
            return false
        }
        return enabled
    }
}

