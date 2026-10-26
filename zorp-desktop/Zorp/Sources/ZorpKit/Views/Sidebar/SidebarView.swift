import SwiftUI

/// Conversations, grouped by project, with the workspace at the foot.
public struct SidebarView: View {
    @Bindable var app: AppState
    @State private var search = ""
    @State private var newProjectName = ""
    @State private var naming = false
    @State private var fileAfterNaming: String?
    @State private var hits: [RecallHit] = []
    @State private var searchNote: String?

    public init(app: AppState) {
        self.app = app
    }

    private var visible: [SessionSummary] {
        let query = search.trimmingCharacters(in: .whitespaces)
        guard !query.isEmpty else { return app.sessions }
        return app.sessions.filter { $0.title.localizedCaseInsensitiveContains(query) }
    }

    public var body: some View {
        VStack(spacing: 0) {
            List(selection: $app.selectedSessionId) {
                if !hits.isEmpty || searchNote != nil {
                    Section("Found in conversations") {
                        if let searchNote {
                            Text(searchNote).font(.caption).foregroundStyle(.secondary)
                        }
                        // Several hits can come from one conversation, so they are
                        // keyed by position rather than by conversation id.
                        ForEach(Array(hits.enumerated()), id: \.offset) { _, hit in
                            VStack(alignment: .leading, spacing: 2) {
                                Text(hit.title).lineLimit(1)
                                Text("\(hit.role == "assistant" ? "Model: " : "You: ")\(hit.snippet)")
                                    .font(.caption).foregroundStyle(.secondary).lineLimit(2)
                            }
                            .tag(hit.id)
                        }
                    }
                }
                ForEach(app.projects) { project in
                    let rows = visible.filter { $0.projectId == project.id }
                    Section {
                        ForEach(rows) { row($0) }
                        if rows.isEmpty {
                            Text("No conversations").font(.caption).foregroundStyle(.tertiary)
                        }
                    } header: {
                        HStack {
                            Label(project.name, systemImage: "folder")
                            Spacer()
                        }
                        .contextMenu {
                            Button("Delete Project", role: .destructive) {
                                Task { await app.deleteProject(project.id) }
                            }
                            .help("Its conversations are kept and moved out of it.")
                        }
                    }
                }
                Section(app.projects.isEmpty ? "Conversations" : "Not in a project") {
                    let rows = visible.filter { row in
                        row.projectId == nil || !app.projects.contains { $0.id == row.projectId }
                    }
                    ForEach(rows) { row($0) }
                    if app.sessions.isEmpty {
                        Text("No conversations yet").font(.caption).foregroundStyle(.tertiary)
                    }
                }
            }
            .listStyle(.sidebar)
            .searchable(text: $search, placement: .sidebar,
                        prompt: app.recall?.available == true ? "Search (Return searches text)" : "Search titles")
            .onSubmit(of: .search) { Task { await searchText() } }
            .onChange(of: search) {
                if search.isEmpty {
                    hits = []
                    searchNote = nil
                }
            }

            Divider()
            Button {
                app.wantsWorkspacePicker = true
            } label: {
                HStack(spacing: 6) {
                    Image(systemName: "folder.badge.gearshape")
                    Text(app.workspace?.path.map { ($0 as NSString).abbreviatingWithTildeInPath } ?? "Choose a workspace…")
                        .lineLimit(1)
                        .truncationMode(.middle)
                    Spacer()
                }
                .font(.caption)
                .foregroundStyle(.secondary)
                .padding(10)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .help("The folder Zorp works in. Click to change it.")
        }
        .toolbar {
            ToolbarItem {
                Button {
                    app.newChat()
                } label: {
                    Label("New Chat", systemImage: "square.and.pencil")
                }
                .help("New chat (⌘N)")
            }
        }
        .alert("New Project", isPresented: $naming) {
            TextField("Name", text: $newProjectName)
            Button("Create") {
                let name = newProjectName
                let session = fileAfterNaming
                newProjectName = ""
                Task {
                    if let project = await app.createProject(name: name), let session {
                        await app.file(session: session, under: project.id)
                    }
                }
            }
            Button("Cancel", role: .cancel) { newProjectName = "" }
        } message: {
            Text("A project groups conversations and limits what their memory reads.")
        }
    }

    /// Search what was said, not just titles, through the local index.
    private func searchText() async {
        let query = search.trimmingCharacters(in: .whitespaces)
        guard !query.isEmpty else { return }
        guard app.recall?.available == true else {
            searchNote = app.recall?.reason ?? "Searching conversation text needs a local Ollama with an embedding model."
            hits = []
            return
        }
        do {
            hits = try await app.client.recallSearch(query: query)
            searchNote = hits.isEmpty ? "Nothing matched." : nil
        } catch {
            hits = []
            searchNote = error.localizedDescription
        }
    }

    @ViewBuilder
    private func row(_ session: SessionSummary) -> some View {
        HStack(spacing: 6) {
            Text(session.title.isEmpty ? "New chat" : session.title)
                .lineLimit(1)
            Spacer()
            if session.status == "running", session.isStored {
                Circle().fill(Color.orange).frame(width: 6, height: 6).help("Running")
            }
        }
        .tag(session.id)
        .contextMenu {
            Menu("Move to Project") {
                ForEach(app.projects) { project in
                    Button(project.name) {
                        Task { await app.file(session: session.id, under: project.id) }
                    }
                    .disabled(session.projectId == project.id)
                }
                if session.projectId != nil {
                    Divider()
                    Button("Remove from Project") {
                        Task { await app.file(session: session.id, under: nil) }
                    }
                }
                Divider()
                Button("New Project…") {
                    fileAfterNaming = session.id
                    naming = true
                }
            }
            .disabled(!session.isStored)
            Divider()
            Button("Delete", role: .destructive) {
                Task { await app.deleteSession(session.id) }
            }
        }
    }
}
