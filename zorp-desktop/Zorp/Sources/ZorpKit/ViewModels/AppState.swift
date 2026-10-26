import Foundation
import Observation

/// The whole window's state: the server, the sidebar, the workspace, the
/// model settings and the conversation on screen.
@MainActor
@Observable
public final class AppState: SessionHost {
    public let client: ZorpClient
    public private(set) var sessions: [SessionSummary] = []
    public private(set) var projects: [Project] = []
    public private(set) var workspace: Workspace?
    public private(set) var settings: Settings?
    public private(set) var models: [ModelDetail] = []
    public private(set) var modelsError: String?
    public private(set) var isLoadingModels = false
    public private(set) var serverReachable = true
    /// The last thing that went wrong outside a conversation, for the
    /// banner. Cleared by whatever fixes it.
    public var banner: String?
    /// The conversation on screen. A new chat has no id until it is sent.
    public private(set) var current: SessionViewModel
    public var selectedSessionId: String? {
        didSet {
            guard selectedSessionId != oldValue else { return }
            if let id = selectedSessionId, id != current.sessionId {
                openSession(id)
            }
        }
    }
    /// Raised when the workspace picker should open.
    public var wantsWorkspacePicker = false
    /// Raised when the settings window should open: no model is set up.
    public var wantsSettings = false
    /// Bumped whenever the Files pane should list again.
    public private(set) var filesGeneration = 0

    public init(baseURL: URL) {
        client = ZorpClient(baseURL: baseURL)
        current = SessionViewModel(client: client)
        current.host = self
    }

    // MARK: launch

    /// Everything the window needs before the first message: the key from
    /// the keychain handed to the server, then the settings, workspace,
    /// sessions and projects.
    public func start() async {
        serverReachable = await client.isReachable()
        guard serverReachable else {
            banner = "The built-in Zorp server is not answering. Quit and reopen Zorp."
            return
        }
        if let key = Keychain.apiKey() {
            _ = try? await client.putSettings(SettingsUpdate(apiKey: key))
        }
        await refreshSettings()
        await refreshWorkspace()
        await refreshSessions()
        await refreshProjects()
        if workspace?.configured != true {
            wantsWorkspacePicker = true
        } else if settings?.configured != true {
            wantsSettings = true
        }
        if let settings, settings.configured {
            await loadModels(baseURL: settings.baseURL)
        }
    }

    // MARK: sessions

    public func newChat() {
        current.close()
        current = SessionViewModel(client: client)
        current.host = self
        selectedSessionId = nil
    }

    public func openSession(_ id: String) {
        guard id != current.sessionId else { return }
        current.close()
        let title = sessions.first { $0.id == id }?.title ?? "Conversation"
        let vm = SessionViewModel(client: client, sessionId: id, title: title)
        vm.host = self
        current = vm
        if selectedSessionId != id { selectedSessionId = id }
        Task { await vm.open() }
    }

    public func refreshSessions() async {
        do {
            sessions = try await client.listSessions()
            if let id = current.sessionId, let row = sessions.first(where: { $0.id == id }),
               current.title != row.title, !row.title.isEmpty, row.title != "New chat" {
                current.title = row.title
            }
        } catch {
            banner = "Could not list conversations: \(error.localizedDescription)"
        }
    }

    public func deleteSession(_ id: String) async {
        do {
            try await client.deleteSession(id: id)
            if current.sessionId == id { newChat() }
            await refreshSessions()
        } catch let error as APIError where error.status == 409 {
            banner = "That conversation is still running. Stop it before deleting it."
        } catch {
            banner = "Could not delete: \(error.localizedDescription)"
        }
    }

    public func branch(at ordinal: Int) async {
        guard let newId = await current.branch(at: ordinal) else { return }
        await refreshSessions()
        openSession(newId)
    }

    // MARK: projects

    public func refreshProjects() async {
        if let list = try? await client.listProjects() { projects = list }
    }

    public func createProject(name: String) async -> Project? {
        do {
            let project = try await client.createProject(name: name)
            await refreshProjects()
            return project
        } catch {
            banner = "Could not create the project: \(error.localizedDescription)"
            return nil
        }
    }

    /// Delete a project. Its conversations are unfiled, never deleted.
    public func deleteProject(_ id: String) async {
        do {
            try await client.deleteProject(id: id)
            await refreshProjects()
            await refreshSessions()
        } catch {
            banner = "Could not delete the project: \(error.localizedDescription)"
        }
    }

    public func file(session id: String, under projectId: String?) async {
        do {
            try await client.setSessionProject(id: id, projectId: projectId)
            await refreshSessions()
        } catch let error as APIError where error.status == 404 {
            banner = "Send a message first. A conversation can be filed once it has one."
        } catch {
            banner = "Could not move the conversation: \(error.localizedDescription)"
        }
    }

    // MARK: workspace

    public func refreshWorkspace() async {
        if let ws = try? await client.getWorkspace() { workspace = ws }
    }

    /// Point the agent at `path`. The server checks it and says why it
    /// refused one.
    @discardableResult
    public func chooseWorkspace(_ path: String) async -> Bool {
        do {
            workspace = try await client.setWorkspace(path: path)
            banner = nil
            filesGeneration += 1
            if settings?.configured != true { wantsSettings = true }
            return true
        } catch {
            banner = error.localizedDescription
            return false
        }
    }

    // MARK: settings

    public func refreshSettings() async {
        if let s = try? await client.getSettings() { settings = s }
    }

    /// Save model settings. A key goes to the keychain and to the server's
    /// memory, never to the settings file.
    @discardableResult
    public func save(_ update: SettingsUpdate) async -> String? {
        if let key = update.apiKey {
            Keychain.setAPIKey(key)
        }
        do {
            settings = try await client.putSettings(update)
            return nil
        } catch {
            return error.localizedDescription
        }
    }

    public func clearAPIKey() async {
        Keychain.setAPIKey(nil)
        _ = try? await client.putSettings(SettingsUpdate(apiKey: ""))
        await refreshSettings()
    }

    public func loadModels(baseURL: String, apiKey: String? = nil) async {
        isLoadingModels = true
        defer { isLoadingModels = false }
        do {
            let list = try await client.listModels(baseURL: baseURL, apiKey: apiKey)
            models = list.entries.filter(\.answersWithText)
            modelsError = list.error
        } catch {
            models = []
            modelsError = error.localizedDescription
        }
    }

    /// Switch the model the next turn uses.
    public func selectModel(_ id: String) async {
        if let error = await save(SettingsUpdate(model: id)) {
            banner = "Could not switch model: \(error)"
        }
    }

    // MARK: SessionHost

    public func sessionCreated(_ id: String) {
        selectedSessionId = id
        Task { await refreshSessions() }
    }

    public func sessionsChanged() {
        Task { await refreshSessions() }
    }

    public func needsWorkspace() {
        wantsWorkspacePicker = true
    }

    public func turnFinished() {
        filesGeneration += 1
    }
}
