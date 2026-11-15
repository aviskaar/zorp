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
        EndpointPreset(name: "Ollama Cloud", provider: .openai, baseURL: "https://ollama.com/v1", needsKey: true),
        EndpointPreset(name: "DeepSeek", provider: .openai, baseURL: "https://api.deepseek.com/v1", needsKey: true),
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
            ConnectionsSettingsView(app: app)
                .tabItem { Label("Connections", systemImage: "point.3.connected.trianglepath.dotted") }
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

/// Saved model connections: several endpoints kept at once, one of them
/// the default for a new conversation. Keys go to the keychain and the
/// server's memory, never the settings file.
struct ConnectionsSettingsView: View {
    @Bindable var app: AppState
    @State private var list: [ModelConnection] = []
    @State private var defaultId: String?
    @State private var keys: [String: String] = [:]
    @State private var selected: String?
    @State private var status: String?
    @State private var busy = false

    var body: some View {
        HSplitView {
            VStack(alignment: .leading, spacing: 6) {
                List(selection: $selected) {
                    ForEach(list) { c in
                        HStack {
                            Text(verbatim: c.name)
                            if c.id == defaultId {
                                Text("default").font(.caption2).foregroundStyle(.secondary)
                            }
                        }
                        .tag(c.id)
                    }
                }
                HStack {
                    Menu {
                        ForEach(EndpointPreset.all) { p in
                            Button(p.name) { add(p) }
                        }
                        Divider()
                        Button("Custom") { add(nil) }
                    } label: { Image(systemName: "plus") }
                        .menuStyle(.borderlessButton).fixedSize()
                    Button {
                        if let id = selected { remove(id) }
                    } label: { Image(systemName: "minus") }
                        .buttonStyle(.borderless)
                        .disabled(selected == nil)
                }
                .padding(6)
            }
            .frame(minWidth: 180, maxWidth: 220)
            Group {
                if let index = list.firstIndex(where: { $0.id == selected }) {
                    editor(index)
                } else {
                    Text("Add a connection with +. A new conversation uses the default; the picker under the composer switches one conversation to another.")
                        .foregroundStyle(.secondary).padding()
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                }
            }
        }
        .onAppear(perform: load)
    }

    private func editor(_ index: Int) -> some View {
        let id = list[index].id
        return Form {
            TextField("Name", text: Binding(
                get: { list[index].label ?? "" },
                set: { list[index].label = $0 }))
            Picker("Provider", selection: Binding(
                get: { list[index].provider ?? .openai },
                set: { list[index].provider = $0 })) {
                ForEach(ModelProvider.allCases) { Text($0.label).tag($0) }
            }
            TextField("Base URL", text: $list[index].baseURL)
            TextField("Model", text: Binding(
                get: { list[index].model ?? "" },
                set: { list[index].model = $0.isEmpty ? nil : $0 }))
            Section("Bring your own key") {
                SecureField(list[index].hasAPIKey ? "Your key (saved, type to replace)" : "Your key (optional)",
                            text: Binding(get: { keys[id] ?? "" }, set: { keys[id] = $0 }))
                TextField("Or read it from an environment variable", text: Binding(
                    get: { list[index].apiKeyEnv ?? "" },
                    set: { list[index].apiKeyEnv = $0.isEmpty ? nil : $0.uppercased() }))
                Text("Each connection uses only its own key, kept in the keychain or read from that variable. It never borrows the main model's key.")
                    .font(.caption).foregroundStyle(.secondary)
            }
            Toggle("Default for new conversations", isOn: Binding(
                get: { defaultId == id },
                set: { defaultId = $0 ? id : nil }))
            HStack {
                if let status { Text(status).font(.caption).foregroundStyle(.red) }
                Spacer()
                Button("Save") { Task { await save() } }
                    .keyboardShortcut(.defaultAction)
                    .disabled(busy)
            }
        }
        .formStyle(.grouped)
    }

    private func load() {
        list = app.settings?.connections ?? []
        defaultId = app.settings?.defaultConnection
        keys = [:]
        if selected == nil { selected = list.first?.id }
    }

    private func add(_ preset: EndpointPreset?) {
        let label = preset?.name ?? "Connection"
        var id = ModelConnection.slug(label)
        if id.isEmpty { id = "connection" }
        var n = 2
        let base = id
        while list.contains(where: { $0.id == id }) { id = "\(base)-\(n)"; n += 1 }
        list.append(ModelConnection(id: id, label: label, provider: preset?.provider ?? .openai,
                                    baseURL: preset?.baseURL ?? "http://localhost:11434/v1"))
        selected = id
    }

    private func remove(_ id: String) {
        list.removeAll { $0.id == id }
        keys[id] = nil
        if defaultId == id { defaultId = nil }
        selected = list.first?.id
    }

    private func save() async {
        busy = true
        defer { busy = false }
        // Only keys somebody typed are sent; an untouched field keeps the
        // saved one.
        let typed = keys.filter { !$0.value.isEmpty }
        status = await app.saveConnections(list, defaultId: defaultId, keys: typed)
        if status == nil { load() }
    }
}
