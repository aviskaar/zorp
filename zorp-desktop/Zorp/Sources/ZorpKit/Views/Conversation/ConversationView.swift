import SwiftUI

/// The conversation on screen and the composer under it.
public struct ConversationView: View {
    @Bindable var app: AppState
    var session: SessionViewModel

    public init(app: AppState, session: SessionViewModel) {
        self.app = app
        self.session = session
    }

    public var body: some View {
        VStack(spacing: 0) {
            if let banner = app.banner {
                BannerView(text: banner) { app.banner = nil }
            }
            if session.items.isEmpty {
                EmptyConversationView(app: app)
            } else {
                ScrollViewReader { proxy in
                    ScrollView {
                        LazyVStack(alignment: .leading, spacing: 14) {
                            ForEach(session.items) { item in
                                TimelineRow(item: item, session: session) { ordinal in
                                    Task { await app.branch(at: ordinal) }
                                }
                                .id(item.id)
                            }
                            if session.isThinking {
                                HStack(spacing: 8) {
                                    ProgressView().controlSize(.small)
                                    Text("Thinking…").font(.callout).foregroundStyle(.secondary)
                                }
                                .id("thinking")
                            }
                            Color.clear.frame(height: 1).id("bottom")
                        }
                        .padding(.horizontal, 24)
                        .padding(.vertical, 18)
                        .frame(maxWidth: 820)
                        .frame(maxWidth: .infinity)
                    }
                    .onChange(of: session.items) {
                        withAnimation(.easeOut(duration: 0.15)) { proxy.scrollTo("bottom", anchor: .bottom) }
                    }
                    .onAppear { proxy.scrollTo("bottom", anchor: .bottom) }
                }
            }
            Divider()
            ComposerView(app: app, session: session)
        }
    }
}

struct BannerView: View {
    let text: String
    let onClose: () -> Void

    var body: some View {
        HStack(alignment: .top) {
            Image(systemName: "exclamationmark.circle.fill").foregroundStyle(.orange)
            Text(text).font(.callout).textSelection(.enabled)
            Spacer()
            Button(action: onClose) { Image(systemName: "xmark") }.buttonStyle(.borderless)
        }
        .padding(10)
        .background(Color.orange.opacity(0.1))
    }
}

struct EmptyConversationView: View {
    @Bindable var app: AppState

    var body: some View {
        VStack(spacing: 14) {
            Spacer()
            Image(systemName: "sparkles").font(.system(size: 34)).foregroundStyle(.secondary)
            Text("What should Zorp work on?").font(.title2)
            if app.workspace?.configured != true {
                Text("Choose a workspace folder first. Zorp reads and writes files there, and nowhere else by default.")
                    .multilineTextAlignment(.center)
                    .foregroundStyle(.secondary)
                    .frame(maxWidth: 420)
                Button("Choose Workspace…") { app.wantsWorkspacePicker = true }
                    .buttonStyle(.borderedProminent)
            } else if app.settings?.configured != true {
                Text("No model is set up yet. Pick a provider and a model to start.")
                    .foregroundStyle(.secondary)
                Button("Set Up a Model…") { app.wantsSettings = true }
                    .buttonStyle(.borderedProminent)
            } else if let path = app.workspace?.path {
                Text("Working in \(path)").font(.callout).foregroundStyle(.secondary)
            }
            Spacer()
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .padding()
    }
}

struct ComposerView: View {
    @Bindable var app: AppState
    @Bindable var session: SessionViewModel
    @State private var text = ""
    @FocusState private var focused: Bool

    private var trimmed: String { text.trimmingCharacters(in: .whitespacesAndNewlines) }

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            if !session.queue.isEmpty {
                VStack(alignment: .leading, spacing: 4) {
                    ForEach(Array(session.queue.enumerated()), id: \.offset) { index, queued in
                        HStack {
                            Image(systemName: "clock").foregroundStyle(.secondary)
                            Text(queued).lineLimit(1).foregroundStyle(.secondary)
                            Spacer()
                            Button {
                                session.queue.remove(at: index)
                            } label: { Image(systemName: "xmark.circle") }
                                .buttonStyle(.borderless)
                                .help("Don't send this one")
                        }
                        .font(.caption)
                    }
                }
            }
            HStack(alignment: .bottom, spacing: 10) {
                ZStack(alignment: .topLeading) {
                    if text.isEmpty {
                        Text(session.isRunning ? "Queue a follow-up…" : "Message Zorp")
                            .foregroundStyle(.tertiary)
                            .padding(.horizontal, 5)
                            .padding(.vertical, 8)
                            .allowsHitTesting(false)
                    }
                    TextEditor(text: $text)
                        .font(.body)
                        .scrollContentBackground(.hidden)
                        .frame(minHeight: 36, maxHeight: 160)
                        .fixedSize(horizontal: false, vertical: true)
                        .focused($focused)
                        .onKeyPress(.return, phases: .down) { press in
                            if press.modifiers.contains(.shift) { return .ignored }
                            submit()
                            return .handled
                        }
                }
                .padding(6)
                .background(Color(nsColor: .textBackgroundColor))
                .clipShape(RoundedRectangle(cornerRadius: 10))
                .overlay(RoundedRectangle(cornerRadius: 10).stroke(Color.secondary.opacity(0.25)))

                if session.isRunning {
                    Button {
                        Task { await session.stop() }
                    } label: {
                        Image(systemName: "stop.circle.fill").font(.system(size: 26)).foregroundStyle(.red)
                    }
                    .buttonStyle(.plain)
                    .keyboardShortcut(.escape, modifiers: [])
                    .help("Stop (Esc)")
                }
                Button(action: submit) {
                    Image(systemName: "arrow.up.circle.fill").font(.system(size: 26))
                }
                .buttonStyle(.plain)
                .foregroundStyle(trimmed.isEmpty ? Color.secondary : Color.accentColor)
                .disabled(trimmed.isEmpty)
                .help(session.isRunning ? "Queue (Return)" : "Send (Return)")
            }
            HStack(spacing: 12) {
                Toggle(isOn: Binding(
                    get: { session.autoApprove },
                    set: { on in Task { await session.setAutoApprove(on) } })
                ) {
                    Text("Allow tool calls without asking").font(.caption)
                }
                .toggleStyle(.checkbox)
                .help("Stands approvals down for this conversation only. A new chat asks again.")
                Spacer()
                if let fraction = session.contextFraction {
                    ContextMeter(fraction: fraction, used: session.usedTokens, limit: session.limitTokens ?? 0)
                } else if session.usedTokens > 0 {
                    Text("\(session.usedTokens) tokens")
                        .font(.caption).foregroundStyle(.secondary).monospacedDigit()
                }
            }
        }
        .padding(12)
        .frame(maxWidth: 820)
        .frame(maxWidth: .infinity)
        .onAppear { focused = true }
    }

    private func submit() {
        let message = trimmed
        guard !message.isEmpty else { return }
        text = ""
        Task { await session.send(message) }
    }
}

struct ContextMeter: View {
    let fraction: Double
    let used: Int64
    let limit: Int64

    var body: some View {
        HStack(spacing: 6) {
            ProgressView(value: fraction)
                .frame(width: 70)
                .tint(fraction > 0.85 ? .red : fraction > 0.6 ? .orange : .accentColor)
            Text("\(Int(fraction * 100))%").font(.caption).monospacedDigit().foregroundStyle(.secondary)
        }
        .help("\(used) of \(limit) tokens in the context window")
    }
}
