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
        .environment(\.workspaceFiles, app.knownFiles)
        .environment(\.openURL, OpenURLAction { url in
            if let path = FileLink.path(of: url) {
                app.openFile(path)
                return .handled
            }
            return .systemAction
        })
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
