import AppKit
import SwiftUI

/// Where people usually point Zorp. A preset fills the form; nothing is
/// saved until Save.
struct EndpointPreset: Identifiable, Hashable {
    let name: String
    let provider: ModelProvider
    let baseURL: String
    let needsKey: Bool
    var id: String { name }

    static let all: [EndpointPreset] = [
        EndpointPreset(name: "OpenRouter", provider: .openai, baseURL: "https://openrouter.ai/api/v1", needsKey: true),
        EndpointPreset(name: "OpenAI", provider: .openai, baseURL: "https://api.openai.com/v1", needsKey: true),
        EndpointPreset(name: "Anthropic", provider: .anthropic, baseURL: "https://api.anthropic.com", needsKey: true),
        EndpointPreset(name: "Ollama (this Mac)", provider: .openai, baseURL: "http://localhost:11434/v1", needsKey: false),
        EndpointPreset(name: "LM Studio (this Mac)", provider: .openai, baseURL: "http://localhost:1234/v1", needsKey: false),
    ]
}

/// The Settings window: the model and the workspace.
public struct SettingsView: View {
    @Bindable var app: AppState

    public init(app: AppState) {
        self.app = app
    }

    public var body: some View {
        TabView {
            ModelSettingsView(app: app)
                .tabItem { Label("Model", systemImage: "cpu") }
            WorkspaceSettingsView(app: app)
                .tabItem { Label("Workspace", systemImage: "folder") }
            AgentsSettingsView(app: app)
                .tabItem { Label("Agents", systemImage: "person.crop.circle") }
            McpSettingsView(app: app)
                .tabItem { Label("MCP", systemImage: "puzzlepiece.extension") }
            DataSettingsView(app: app)
                .tabItem { Label("Data", systemImage: "internaldrive") }
            DoctorSettingsView(app: app)
                .tabItem { Label("Doctor", systemImage: "stethoscope") }
        }
        .frame(width: 660, height: 580)
    }
}

struct ModelSettingsView: View {
    @Bindable var app: AppState
    @State private var provider: ModelProvider = .openai
    @State private var baseURL = ""
    @State private var apiKey = ""
    @State private var model = ""
    @State private var maxTokens = ""
    @State private var filter = ""
    @State private var freeOnly = false
    @State private var status: String?
    @State private var statusIsError = false
    @State private var busy = false
    @State private var loaded = false

    private var shownModels: [ModelDetail] {
        let query = filter.trimmingCharacters(in: .whitespaces)
        return app.models.filter { detail in
            (!freeOnly || detail.isFree)
                && (query.isEmpty || detail.id.localizedCaseInsensitiveContains(query)
                    || (detail.name?.localizedCaseInsensitiveContains(query) ?? false))
        }
    }

    var body: some View {
        Form {
            Section("Provider") {
                Picker("Start from", selection: Binding<EndpointPreset?>(
                    get: { EndpointPreset.all.first { $0.baseURL == baseURL && $0.provider == provider } },
                    set: { preset in
                        guard let preset else { return }
                        provider = preset.provider
                        baseURL = preset.baseURL
                        Task { await refreshModels() }
                    })
                ) {
                    Text("Custom").tag(EndpointPreset?.none)
                    ForEach(EndpointPreset.all) { Text($0.name).tag(EndpointPreset?.some($0)) }
                }
                Picker("Wire format", selection: $provider) {
                    ForEach(ModelProvider.allCases) { Text($0.label).tag($0) }
                }
                TextField("Base URL", text: $baseURL, prompt: Text("https://openrouter.ai/api/v1"))
                    .onSubmit { Task { await refreshModels() } }
                SecureField("API key", text: $apiKey, prompt: Text(app.settings?.hasAPIKey == true ? "Saved in your keychain" : "Not needed for a local model"))
                if app.settings?.hasAPIKey == true {
                    HStack {
                        Text(keySourceText).font(.caption).foregroundStyle(.secondary)
                        Spacer()
                        if app.settings?.apiKeySource == .ui {
                            Button("Remove Key") { Task { await app.clearAPIKey() } }
                                .controlSize(.small)
                        }
                    }
                }
            }

            Section {
                HStack {
                    TextField("Filter", text: $filter, prompt: Text("Filter models"))
                    Toggle("Free only", isOn: $freeOnly).toggleStyle(.checkbox)
                    Button {
                        Task { await refreshModels() }
                    } label: {
                        if app.isLoadingModels { ProgressView().controlSize(.small) } else { Text("Load Models") }
                    }
                    .disabled(baseURL.isEmpty || app.isLoadingModels)
                }
                if let error = app.modelsError {
                    Text(error).font(.caption).foregroundStyle(.red)
                }
                List(shownModels, selection: Binding<String?>(get: { model.isEmpty ? nil : model }, set: { model = $0 ?? model })) { detail in
                    HStack {
                        VStack(alignment: .leading, spacing: 1) {
                            Text(detail.name ?? detail.id)
                            if detail.name != nil {
                                Text(detail.id).font(.caption2).foregroundStyle(.secondary)
                            }
                        }
                        Spacer()
                        if let context = detail.contextLength {
                            Text("\(context / 1000)k").font(.caption).foregroundStyle(.secondary).monospacedDigit()
                        }
                        if detail.isFree {
                            Text("free").font(.caption2).padding(.horizontal, 5).padding(.vertical, 1)
                                .background(Color.green.opacity(0.15)).clipShape(Capsule())
                        }
                    }
                    .tag(detail.id)
                }
                .frame(minHeight: 160)
                TextField("Model", text: $model, prompt: Text("Pick above, or type an id"))
                TextField("Max tokens", text: $maxTokens, prompt: Text("Provider default"))
            } header: {
                Text("Model")
            }

            Section {
                HStack {
                    if let status {
                        Label(status, systemImage: statusIsError ? "xmark.octagon" : "checkmark.circle")
                            .foregroundStyle(statusIsError ? Color.red : Color.green)
                            .font(.callout)
                            .lineLimit(3)
                    }
                    Spacer()
                    Button("Test Connection") { Task { await test() } }
                        .disabled(busy || baseURL.isEmpty || model.isEmpty)
                    Button("Save") { Task { await save() } }
                        .buttonStyle(.borderedProminent)
                        .disabled(busy || baseURL.isEmpty || model.isEmpty)
                        .keyboardShortcut(.return, modifiers: [.command])
                }
            }
        }
        .formStyle(.grouped)
        .task {
            guard !loaded else { return }
            loaded = true
            await app.refreshSettings()
            if let s = app.settings {
                provider = s.provider
                baseURL = s.baseURL
                model = s.model
                maxTokens = s.maxTokens.map(String.init) ?? ""
            }
            if !baseURL.isEmpty { await refreshModels() }
        }
    }

    private var keySourceText: String {
        switch app.settings?.apiKeySource {
        case .env?: return "Using ZORP_API_KEY from the environment."
        case .ui?: return "A key is saved in your keychain."
        default: return "A key is set."
        }
    }

    private func refreshModels() async {
        guard !baseURL.isEmpty else { return }
        await app.loadModels(baseURL: baseURL, apiKey: apiKey.isEmpty ? nil : apiKey)
    }

    private func test() async {
        busy = true
        defer { busy = false }
        do {
            let result = try await app.client.testConnection(
                provider: provider, baseURL: baseURL, model: model, apiKey: apiKey.isEmpty ? nil : apiKey)
            status = result.ok ? "The model answered." : (result.reason ?? "The model did not answer.")
            statusIsError = !result.ok
        } catch {
            status = error.localizedDescription
            statusIsError = true
        }
    }

    private func save() async {
        busy = true
        defer { busy = false }
        var update = SettingsUpdate(provider: provider, baseURL: baseURL.trimmingCharacters(in: .whitespaces), model: model)
        if let tokens = Int(maxTokens.trimmingCharacters(in: .whitespaces)), tokens > 0 {
            update.maxTokens = tokens
        }
        if !apiKey.isEmpty { update.apiKey = apiKey }
        if let error = await app.save(update) {
            status = error
            statusIsError = true
        } else {
            apiKey = ""
            status = "Saved. The next message uses \(model)."
            statusIsError = false
            app.wantsSettings = false
        }
    }
}

struct WorkspaceSettingsView: View {
    @Bindable var app: AppState

    var body: some View {
        Form {
            Section("Workspace") {
                LabeledContent("Folder") {
                    Text(app.workspace?.path ?? "None chosen").textSelection(.enabled)
                }
                if let scratch = app.workspace?.scratch {
                    LabeledContent("Generated files") { Text(scratch).textSelection(.enabled) }
                }
                if let source = app.workspace?.source, source == "env" || source == "flag" {
                    Text("Set by \(source == "env" ? "ZORP_WORKSPACE" : "--workspace"), which wins over a folder chosen here.")
                        .font(.caption).foregroundStyle(.secondary)
                }
                HStack {
                    Spacer()
                    if let path = app.workspace?.path {
                        Button("Show in Finder") {
                            NSWorkspace.shared.activateFileViewerSelecting([URL(fileURLWithPath: path)])
                        }
                    }
                    Button("Choose…") { WorkspacePicker.run(app: app) }
                }
            }
            Section {
                Text("Zorp runs commands and writes files inside this folder. It asks before running a command unless you allow tool calls for a conversation.")
                    .font(.callout).foregroundStyle(.secondary)
            }
        }
        .formStyle(.grouped)
    }
}

/// The native folder picker, handing its answer to the server, which
/// checks the path the same way wherever it came from.
@MainActor
enum WorkspacePicker {
    static func run(app: AppState) {
        let panel = NSOpenPanel()
        panel.title = "Choose a Workspace"
        panel.message = "Zorp works inside this folder."
        panel.prompt = "Use This Folder"
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.canCreateDirectories = true
        panel.allowsMultipleSelection = false
        if let path = app.workspace?.path {
            panel.directoryURL = URL(fileURLWithPath: path)
        }
        app.wantsWorkspacePicker = false
        guard panel.runModal() == .OK, let url = panel.url else { return }
        Task { await app.chooseWorkspace(url.path) }
    }
}
