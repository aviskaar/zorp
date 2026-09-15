import Foundation
import Observation

@Observable
public final class AppState: @unchecked Sendable {
    public var baseURL: URL
    public var client: ZorpClient
    public var sessions: [SessionSummary] = []
    public var projects: [ProjectItem] = []
    public var activeSessionId: String?
    public var activeSessionVM: SessionViewModel?
    public var workspacePath: String?
    public var skillsCount: Int = 0

    public init(baseURL: URL) {
        self.baseURL = baseURL
        self.client = ZorpClient(baseURL: baseURL)
    }

    public func selectSession(_ id: String) {
        activeSessionVM?.disconnect()
        activeSessionId = id
        let vm = SessionViewModel(sessionId: id, client: client, baseURL: baseURL)
        activeSessionVM = vm
        vm.connect()
    }

    public func createNewSession() async {
        do {
            let id = try await client.createSession()
            await refreshSessions()
            await MainActor.run {
                self.selectSession(id)
            }
        } catch {
            print("Failed to create session: \(error)")
        }
    }

    public func refreshSessions() async {
        do {
            let list = try await client.listSessions()
            await MainActor.run {
                self.sessions = list
            }
        } catch {
            print("Failed to refresh sessions: \(error)")
        }
    }

    public func refreshProjects() async {
        do {
            let list = try await client.listProjects()
            await MainActor.run {
                self.projects = list
            }
        } catch {
            print("Failed to refresh projects: \(error)")
        }
    }

    public func deleteSession(_ id: String) async {
        do {
            try await client.deleteSession(id: id)
            if activeSessionId == id {
                await MainActor.run {
                    activeSessionVM?.disconnect()
                    activeSessionVM = nil
                    activeSessionId = nil
                }
            }
            await refreshSessions()
        } catch {
            print("Failed to delete session: \(error)")
        }
    }

    public func createProject(name: String) async {
        do {
            _ = try await client.createProject(name: name)
            await refreshProjects()
        } catch {
            print("Failed to create project: \(error)")
        }
    }

    public func setSessionProject(sessionId: String, projectId: String?) async {
        do {
            try await client.setSessionProject(sessionId: sessionId, projectId: projectId)
            await refreshSessions()
        } catch {
            print("Failed to set session project: \(error)")
        }
    }
}
