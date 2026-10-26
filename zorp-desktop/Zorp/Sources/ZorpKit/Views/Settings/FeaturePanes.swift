import AppKit
import SwiftUI

/// The agent the conversation runs under. A flavor with a description,
/// chosen by a person; locked once the conversation has answered.
struct AgentMenu: View {
    @Bindable var app: AppState

    private var current: String {
        app.pendingAgent ?? app.current.agent ?? app.agentListing?.defaultName ?? "Default"
    }

    var body: some View {
        Menu {
            Button("Default agent") { Task { await app.chooseAgent(nil) } }
            if let agents = app.agentListing?.agents, !agents.isEmpty {
                Divider()
                ForEach(agents) { agent in
                    Button {
                        Task { await app.chooseAgent(agent.name) }
                    } label: {
                        Text(agent.broken.map { "\(agent.name) (broken: \($0))" } ?? agent.name)
                    }
                    .disabled(agent.broken != nil)
                }
            }
        } label: {
            Label(current, systemImage: "person.crop.circle")
        }
        .menuStyle(.borderlessButton)
        .fixedSize()
        .help("The agent this conversation runs under. Fixed once it has answered; branch to change it.")
    }
}

/// What skills are installed and which are in this conversation's
/// context. Read only: loading a skill is the model's `skill` tool.
struct SkillsPopover: View {
    @Bindable var app: AppState
    @State private var active: ActiveSkillListing?

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("Skills").font(.headline)
            if let offer = app.skillListing?.offer, !offer.withheld.isEmpty {
                Text(offer.reason).font(.caption).foregroundStyle(.secondary)
            }
            if let skills = app.skillListing?.skills, !skills.isEmpty {
                ForEach(skills) { skill in
                    let withheld = app.skillListing?.offer?.withheld.contains(skill.name) == true
                    let state = active?.skills.first { $0.name == skill.name }
                    HStack(alignment: .firstTextBaseline) {
                        Image(systemName: state?.active == true ? "checkmark.circle.fill" : "circle")
                            .foregroundStyle(state?.active == true ? Color.green : Color.secondary)
                        VStack(alignment: .leading, spacing: 1) {
                            Text(skill.name).bold()
                            Text(skill.description).font(.caption).foregroundStyle(.secondary).lineLimit(2)
                        }
                        Spacer()
                        Text(skill.scope).font(.caption2).foregroundStyle(.tertiary)
                    }
                    .opacity(withheld ? 0.5 : 1)
                }
                Text("A checkmark means the skill's instructions are in this conversation's context. The model loads skills itself when a task matches.")
                    .font(.caption2).foregroundStyle(.secondary)
            } else {
                Text("No skills installed.").foregroundStyle(.secondary)
            }
        }
        .padding()
        .frame(width: 360)
        .task {
            if let id = app.current.sessionId {
                active = try? await app.client.activeSkills(id: id)
            }
        }
    }
}

struct AgentsSettingsView: View {
    @Bindable var app: AppState

    var body: some View {
        Form {
            if let agents = app.agentListing?.agents, !agents.isEmpty {
                ForEach(agents) { agent in
                    Section(agent.name) {
                        if let description = agent.description { Text(description) }
                        LabeledContent("Scope", value: agent.scope)
                        if let model = agent.model { LabeledContent("Model", value: model) }
                        if let broken = agent.broken {
                            Text(broken).foregroundStyle(.red)
                        }
                        if agent.wantsPrivilege {
                            VStack(alignment: .leading, spacing: 4) {
                                Text(agent.trusted
                                    ? "Trusted. Its privileged settings apply."
                                    : "Asks for more than an ordinary agent. Its privileged settings are ignored until you trust it, and editing the file revokes that.")
                                    .font(.caption).foregroundStyle(agent.trusted ? Color.secondary : Color.orange)
                                ForEach(agent.privilegeSummary, id: \.self) { line in
                                    Text("• \(line)").font(.caption)
                                }
                                if !agent.trusted {
                                    Button("Trust \(agent.name)") { Task { await app.trust(agent: agent) } }
                                }
                            }
                        }
                    }
                }
            } else {
                Text("No agents. Put a flavor file in ~/.zorp/flavors/ or in the workspace's .zorp/flavors/ to add one.")
                    .foregroundStyle(.secondary)
            }
        }
        .formStyle(.grouped)
        .task { await app.refreshCapabilities() }
    }
}

struct McpSettingsView: View {
    @Bindable var app: AppState
    @State private var listing: McpListing?
    @State private var error: String?

    var body: some View {
        Form {
            if let listing {
                Section {
                    Text(listing.why).font(.callout)
                    if let warning = listing.warning { Text(warning).foregroundStyle(.orange) }
                    ForEach(listing.sources, id: \.self) { Text($0).font(.caption).foregroundStyle(.secondary) }
                }
                ForEach(listing.servers) { server in
                    Section(server.name) {
                        LabeledContent("Transport", value: server.transport)
                        if let command = server.command { LabeledContent("Command", value: command.joined(separator: " ")) }
                        if let url = server.url { LabeledContent("URL", value: url) }
                        LabeledContent("Trust", value: server.trust)
                        LabeledContent("Loaded", value: server.loaded ? "yes" : "no")
                    }
                }
            } else if let error {
                Text(error).foregroundStyle(.red)
            } else {
                ProgressView()
            }
        }
        .formStyle(.grouped)
        .task {
            do { listing = try await app.client.mcp() } catch { self.error = error.localizedDescription }
        }
    }
}

struct DataSettingsView: View {
    @Bindable var app: AppState
    @State private var state: DataState?
    @State private var confirm: String?

    var body: some View {
        Form {
            if let state {
                Section("What Zorp keeps on this Mac") {
                    ForEach(state.files) { file in
                        VStack(alignment: .leading, spacing: 2) {
                            HStack {
                                Text(file.label).bold()
                                Spacer()
                                Text(file.exists ? ByteCountFormatter.string(fromByteCount: file.bytes ?? 0, countStyle: .file) : "not created")
                                    .font(.caption).foregroundStyle(.secondary)
                            }
                            Text(file.what).font(.caption).foregroundStyle(.secondary)
                            Text(file.path).font(.caption2).foregroundStyle(.tertiary).textSelection(.enabled)
                        }
                    }
                }
                Section {
                    Button("Delete All Conversations…", role: .destructive) { confirm = "conversations" }
                    Button("Delete the Search Index…", role: .destructive) { confirm = "index" }
                    Button("Reset Settings…", role: .destructive) { confirm = "settings" }
                    Text(state.resetNote).font(.caption).foregroundStyle(.secondary)
                    Text("None of these touch a file in your workspace.").font(.caption).foregroundStyle(.secondary)
                }
            } else {
                ProgressView()
            }
        }
        .formStyle(.grouped)
        .task { await load() }
        .confirmationDialog("This cannot be undone.", isPresented: Binding(get: { confirm != nil }, set: { if !$0 { confirm = nil } })) {
            Button("Delete", role: .destructive) {
                let what = confirm
                confirm = nil
                Task {
                    switch what {
                    case "conversations": await app.clearAllConversations()
                    case "index": await app.deleteSearchIndex()
                    case "settings": await app.resetSettings()
                    default: break
                    }
                    await load()
                }
            }
        }
    }

    private func load() async {
        state = try? await app.client.dataState()
    }
}

struct DoctorSettingsView: View {
    @Bindable var app: AppState
    @State private var report: DoctorReport?
    @State private var busy = false

    var body: some View {
        Form {
            if let report {
                Section {
                    LabeledContent("Version", value: report.version)
                    LabeledContent("Endpoint", value: report.baseURL)
                    Label(report.healthy ? "Everything checked is fine." : "Something needs attention.",
                          systemImage: report.healthy ? "checkmark.seal" : "exclamationmark.triangle")
                        .foregroundStyle(report.healthy ? Color.green : Color.orange)
                }
                Section("Checks") {
                    ForEach(Array(report.checks.enumerated()), id: \.offset) { _, check in
                        HStack(alignment: .firstTextBaseline) {
                            Image(systemName: symbol(check.health)).foregroundStyle(color(check.health))
                            VStack(alignment: .leading, spacing: 1) {
                                Text(check.label)
                                Text(check.detail).font(.caption).foregroundStyle(.secondary).textSelection(.enabled)
                            }
                        }
                    }
                }
            } else {
                ProgressView()
            }
            Section {
                HStack {
                    Spacer()
                    Button("Copy Report") {
                        guard let report else { return }
                        let text = report.checks.map { "[\($0.health)] \($0.label): \($0.detail)" }.joined(separator: "\n")
                        NSPasteboard.general.clearContents()
                        NSPasteboard.general.setString("zorp \(report.version)\n\(text)", forType: .string)
                    }
                    .disabled(report == nil)
                    Button(busy ? "Checking…" : "Check the Endpoint") { Task { await load(probe: true) } }
                        .disabled(busy)
                }
                Text("The report never contains a key, only whether one is set.").font(.caption).foregroundStyle(.secondary)
            }
        }
        .formStyle(.grouped)
        .task { await load(probe: false) }
    }

    private func load(probe: Bool) async {
        busy = true
        defer { busy = false }
        report = try? await app.client.doctor(probe: probe)
    }

    private func symbol(_ health: String) -> String {
        switch health {
        case "ok": return "checkmark.circle.fill"
        case "bad": return "xmark.octagon.fill"
        case "off": return "minus.circle"
        default: return "info.circle"
        }
    }

    private func color(_ health: String) -> Color {
        switch health {
        case "ok": return .green
        case "bad": return .red
        default: return .secondary
        }
    }
}
