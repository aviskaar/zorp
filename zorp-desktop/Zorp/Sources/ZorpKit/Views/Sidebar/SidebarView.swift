import SwiftUI

public struct SidebarView: View {
    @Bindable public var appState: AppState
    @State private var searchText: String = ""

    public init(appState: AppState) {
        self.appState = appState
    }

    private var filteredSessions: [SessionSummary] {
        if searchText.trimmingCharacters(in: .whitespaces).isEmpty {
            return appState.sessions
        }
        return appState.sessions.filter {
            $0.title.localizedCaseInsensitiveContains(searchText)
        }
    }

    public var body: some View {
        VStack(spacing: 0) {
            List(selection: $appState.activeSessionId) {
                if !appState.projects.isEmpty {
                    Section("Projects") {
                        ForEach(appState.projects) { project in
                            DisclosureGroup {
                                let projectSessions = filteredSessions.filter { $0.projectId == project.id }
                                ForEach(projectSessions) { session in
                                    sessionRow(session)
                                }
                            } label: {
                                Label(project.name, systemImage: "folder.fill")
                                    .font(.subheadline)
                            }
                        }
                    }
                }

                Section("Recent Sessions") {
                    let unassigned = filteredSessions.filter { $0.projectId == nil }
                    if unassigned.isEmpty && filteredSessions.isEmpty {
                        Text("No sessions found")
                            .font(.caption)
                            .foregroundColor(.secondary)
                    } else {
                        ForEach(unassigned) { session in
                            sessionRow(session)
                        }
                    }
                }
            }
            .searchable(text: $searchText, placement: .sidebar)
            .toolbar {
                ToolbarItem(placement: .primaryAction) {
                    Button(action: {
                        Task {
                            await appState.createNewSession()
                        }
                    }) {
                        Image(systemName: "square.and.pencil")
                    }
                    .help("New Session (⌘N)")
                }
            }

            Divider()

            // Footer with workspace path and skill count
            HStack(spacing: 8) {
                Image(systemName: "folder")
                    .foregroundColor(.secondary)
                Text(appState.workspacePath ?? "Workspace")
                    .font(.caption)
                    .lineLimit(1)
                    .truncationMode(.middle)
                    .foregroundColor(.secondary)

                Spacer()

                if appState.skillsCount > 0 {
                    HStack(spacing: 3) {
                        Image(systemName: "sparkles")
                        Text("\(appState.skillsCount)")
                    }
                    .font(.caption2)
                    .padding(.horizontal, 6)
                    .padding(.vertical, 2)
                    .background(Color.secondary.opacity(0.15))
                    .cornerRadius(4)
                }
            }
            .padding(10)
            .background(Color(.windowBackgroundColor).opacity(0.8))
        }
        .task {
            await appState.refreshSessions()
            await appState.refreshProjects()
        }
    }

    @ViewBuilder
    private func sessionRow(_ session: SessionSummary) -> some View {
        HStack {
            VStack(alignment: .leading, spacing: 2) {
                Text(session.title.isEmpty ? "New Chat" : session.title)
                    .font(.body)
                    .lineLimit(1)
                if session.status == "running" {
                    Text("In progress...")
                        .font(.caption2)
                        .foregroundColor(.accentColor)
                }
            }
            Spacer()
        }
        .tag(session.id)
        .contextMenu {
            Button(role: .destructive) {
                Task {
                    await appState.deleteSession(session.id)
                }
            } label: {
                Label("Delete Session", systemImage: "trash")
            }
        }
    }
}
