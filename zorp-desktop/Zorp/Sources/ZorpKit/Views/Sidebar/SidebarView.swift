import SwiftUI

/// Conversations, grouped by project, with the workspace at the foot.
public struct SidebarView: View {
    @Bindable var app: AppState
    @State private var search = ""
    @State private var newProjectName = ""
    @State private var naming = false
    @State private var fileAfterNaming: String?

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
            .searchable(text: $search, placement: .sidebar, prompt: "Search titles")

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
