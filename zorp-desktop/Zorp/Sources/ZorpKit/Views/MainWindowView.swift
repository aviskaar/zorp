import SwiftUI

public struct MainWindowView: View {
    @Bindable var app: AppState
    @State private var showFiles = false
    @Environment(\.openSettings) private var openSettings

    public init(app: AppState) {
        self.app = app
    }

    public var body: some View {
        NavigationSplitView {
            SidebarView(app: app)
                .navigationSplitViewColumnWidth(min: 220, ideal: 260, max: 380)
        } detail: {
            ConversationView(app: app, session: app.current)
                .navigationTitle(app.current.title)
                .navigationSubtitle(app.workspace?.path.map { ($0 as NSString).abbreviatingWithTildeInPath } ?? "")
        }
        .inspector(isPresented: $showFiles) {
            FilesPane(app: app)
                .inspectorColumnWidth(min: 260, ideal: 340, max: 600)
        }
        .toolbar {
            ToolbarItem(placement: .principal) {
                ModelMenu(app: app) { openSettings() }
            }
            ToolbarItem(placement: .primaryAction) {
                Button {
                    showFiles.toggle()
                } label: {
                    Label("Files", systemImage: "sidebar.trailing")
                }
                .help("Show the workspace's files")
            }
        }
        .task { await app.start() }
        .onChange(of: app.wantsWorkspacePicker) {
            if app.wantsWorkspacePicker { WorkspacePicker.run(app: app) }
        }
        .onChange(of: app.wantsSettings) {
            if app.wantsSettings {
                app.wantsSettings = false
                openSettings()
            }
        }
        .onChange(of: app.filesGeneration) {
            // A turn that wrote files is worth a look; open the pane once.
            if !showFiles, app.current.items.contains(where: { $0.isTool }) {
                Task {
                    if let listing = try? await app.client.listArtifacts(), !listing.files.isEmpty {
                        showFiles = true
                    }
                }
            }
        }
    }
}

/// The model the next message goes to, switchable from the toolbar.
struct ModelMenu: View {
    @Bindable var app: AppState
    let openSettings: () -> Void

    private var current: String {
        guard let settings = app.settings else { return "No model" }
        return settings.configured ? settings.model : "Set up a model"
    }

    var body: some View {
        Menu {
            if app.models.isEmpty {
                Text(app.modelsError ?? "No models listed yet")
            } else {
                ForEach(app.models.prefix(60)) { detail in
                    Button {
                        Task { await app.selectModel(detail.id) }
                    } label: {
                        if detail.id == app.settings?.model {
                            Label(detail.name ?? detail.id, systemImage: "checkmark")
                        } else {
                            Text(detail.name ?? detail.id)
                        }
                    }
                }
                if app.models.count > 60 {
                    Text("\(app.models.count - 60) more in Settings")
                }
            }
            Divider()
            Button("Model Settings…", action: openSettings)
        } label: {
            Label(current, systemImage: "cpu")
                .labelStyle(.titleAndIcon)
        }
        .menuStyle(.borderlessButton)
        .fixedSize()
        .help("The model the next message is sent to")
    }
}
