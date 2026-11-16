import AppKit
import SwiftUI

/// The guided first run: shown on a fresh install and after Reset Settings.
/// It is a client for the same settings and workspace routes the Settings
/// window uses and holds nothing of its own; closing it early leaves the
/// app exactly as configured as it was.
struct OnboardingView: View {
    @Bindable var app: AppState
    @State private var step: Step = .welcome
    @State private var forward = true
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    enum Step: Int, CaseIterable {
        case welcome, workspace, model, done
    }

    var body: some View {
        ZStack {
            AuroraBackground(paused: reduceMotion)
            VStack(spacing: 0) {
                HStack {
                    StepDots(step: step)
                    Spacer()
                    if step != .done {
                        Button("Skip for Now") { finish() }
                            .buttonStyle(.borderless)
                            .foregroundStyle(.secondary)
                    }
                }
                .padding(.horizontal, 28)
                .padding(.top, 22)

                ZStack {
                    page
                        .id(step)
                        .transition(reduceMotion ? .opacity : .asymmetric(
                            insertion: .move(edge: forward ? .trailing : .leading).combined(with: .opacity),
                            removal: .move(edge: forward ? .leading : .trailing).combined(with: .opacity)))
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                .clipped()
            }
            .frame(maxWidth: 640, maxHeight: 600)
            .background(.regularMaterial, in: RoundedRectangle(cornerRadius: 22))
            .overlay(RoundedRectangle(cornerRadius: 22).stroke(.white.opacity(0.12)))
            .shadow(color: .black.opacity(0.25), radius: 30, y: 12)
            .padding(32)
        }
        .onAppear { step = .welcome }
    }

    @ViewBuilder
    private var page: some View {
        switch step {
        case .welcome: WelcomePage { go(.workspace) }
        case .workspace: WorkspacePage(app: app, back: { go(.welcome) }) { go(.model) }
        case .model: ModelPage(app: app, back: { go(.workspace) }) { go(.done) }
        case .done: DonePage(app: app) { finish() }
        }
    }

    private func go(_ next: Step) {
        forward = next.rawValue > step.rawValue
        withAnimation(.spring(response: 0.5, dampingFraction: 0.86)) { step = next }
    }

    private func finish() {
        withAnimation(.easeInOut(duration: 0.35)) { app.showOnboarding = false }
    }
}

// MARK: pages

private struct WelcomePage: View {
    let next: () -> Void
    @State private var shown = false

    var body: some View {
        VStack(spacing: 22) {
            Spacer()
            OrbitingIcon()
                .frame(width: 150, height: 150)
            VStack(spacing: 8) {
                Text("Welcome to Zorp")
                    .font(.system(size: 34, weight: .bold, design: .rounded))
                Text("Ask hard questions. Get answers backed by evidence, worked out in a folder you choose.")
                    .font(.title3)
                    .foregroundStyle(.secondary)
                    .multilineTextAlignment(.center)
                    .frame(maxWidth: 440)
            }
            .opacity(shown ? 1 : 0)
            .offset(y: shown ? 0 : 12)
            Spacer()
            Button(action: next) {
                Label("Get Started", systemImage: "arrow.right")
                    .labelStyle(TrailingIconLabelStyle())
                    .frame(minWidth: 160)
            }
            .buttonStyle(.borderedProminent)
            .controlSize(.large)
            .keyboardShortcut(.defaultAction)
            .opacity(shown ? 1 : 0)
            .padding(.bottom, 34)
        }
        .padding(.horizontal, 40)
        .onAppear { withAnimation(.easeOut(duration: 0.7).delay(0.15)) { shown = true } }
    }
}

private struct WorkspacePage: View {
    @Bindable var app: AppState
    let back: () -> Void
    let next: () -> Void
    @State private var pulse = 0

    private var configured: Bool { app.workspace?.configured == true }

    var body: some View {
        PageScaffold(
            symbol: configured ? "folder.fill" : "folder.badge.plus",
            symbolTrigger: pulse,
            title: "Pick a workspace",
            subtitle: "Zorp reads, runs commands and writes files only inside this folder. Generated files go in its scratch folder.",
            back: back
        ) {
            VStack(spacing: 14) {
                if let path = app.workspace?.path, configured {
                    HStack(spacing: 10) {
                        Image(systemName: "checkmark.circle.fill").foregroundStyle(.green)
                        Text(verbatim: (path as NSString).abbreviatingWithTildeInPath)
                            .font(.callout.monospaced())
                            .lineLimit(1).truncationMode(.middle)
                        Spacer()
                        Button("Change…") { WorkspacePicker.run(app: app) }
                    }
                    .padding(12)
                    .background(.background.opacity(0.6), in: RoundedRectangle(cornerRadius: 10))
                    .transition(.scale(scale: 0.9).combined(with: .opacity))
                } else {
                    Button {
                        WorkspacePicker.run(app: app)
                    } label: {
                        Label("Choose a Folder…", systemImage: "folder")
                            .frame(minWidth: 200)
                    }
                    .controlSize(.large)
                }
                if let banner = app.banner {
                    Text(verbatim: banner).font(.caption).foregroundStyle(.red)
                }
            }
            .animation(.spring(response: 0.45, dampingFraction: 0.8), value: configured)
        } footer: {
            Button("Continue", action: next)
                .buttonStyle(.borderedProminent)
                .controlSize(.large)
                .disabled(!configured)
                .keyboardShortcut(.defaultAction)
        }
        .onChange(of: app.workspace?.path) { pulse += 1 }
    }
}

private struct ModelPage: View {
    @Bindable var app: AppState
    let back: () -> Void
    let next: () -> Void
    @State private var preset: EndpointPreset?
    @State private var baseURL = ""
    @State private var provider: ModelProvider = .openai
    @State private var apiKey = ""
    @State private var model = ""
    @State private var filter = ""
    @State private var status: String?
    @State private var failed = false
    @State private var busy = false

    private let columns = [GridItem(.adaptive(minimum: 150), spacing: 10)]

    private var shown: [ModelDetail] {
        let q = filter.trimmingCharacters(in: .whitespaces)
        return app.models.filter {
            q.isEmpty || $0.id.localizedCaseInsensitiveContains(q) || ($0.name?.localizedCaseInsensitiveContains(q) ?? false)
        }
    }

    var body: some View {
        PageScaffold(
            symbol: "cpu",
            symbolTrigger: preset?.id,
            title: "Connect a model",
            subtitle: "A hosted provider with your own key, or a model running on this Mac. The key goes to your keychain, never a file.",
            back: back
        ) {
            VStack(alignment: .leading, spacing: 12) {
                LazyVGrid(columns: columns, spacing: 10) {
                    ForEach(EndpointPreset.all) { p in
                        PresetCard(preset: p, selected: preset == p) { choose(p) }
                    }
                }
                if let preset {
                    VStack(alignment: .leading, spacing: 8) {
                        if preset.needsKey {
                            SecureField("API key", text: $apiKey, prompt: Text(app.settings?.hasAPIKey == true ? "Saved in your keychain" : "Paste your \(preset.name) key"))
                                .onSubmit { Task { await load() } }
                        }
                        HStack {
                            TextField("Filter", text: $filter, prompt: Text("Filter models"))
                            Button {
                                Task { await load() }
                            } label: {
                                if app.isLoadingModels { ProgressView().controlSize(.small) } else { Text("Load Models") }
                            }
                            .disabled(app.isLoadingModels)
                        }
                        if let error = app.modelsError {
                            Text(verbatim: error).font(.caption).foregroundStyle(.red).lineLimit(2)
                        }
                        if !app.models.isEmpty {
                            List(shown.prefix(200), selection: Binding<String?>(get: { model.isEmpty ? nil : model }, set: { model = $0 ?? model })) { d in
                                HStack {
                                    Text(verbatim: d.name ?? d.id).lineLimit(1)
                                    Spacer()
                                    if d.isFree {
                                        Text("free").font(.caption2).foregroundStyle(.green)
                                    }
                                }
                                .tag(d.id)
                            }
                            .frame(height: 110)
                            .clipShape(RoundedRectangle(cornerRadius: 8))
                        }
                        TextField("Model", text: $model, prompt: Text("Pick above, or type a model id"))
                    }
                    .transition(.move(edge: .bottom).combined(with: .opacity))
                }
                if let status {
                    Label { Text(verbatim: status) } icon: {
                        Image(systemName: failed ? "xmark.octagon.fill" : "checkmark.circle.fill")
                    }
                    .foregroundStyle(failed ? .red : .green)
                    .font(.callout)
                    .lineLimit(2)
                    .transition(.opacity)
                }
            }
            .animation(.spring(response: 0.45, dampingFraction: 0.85), value: preset)
            .animation(.easeInOut(duration: 0.2), value: status)
        } footer: {
            Button {
                Task { await connect() }
            } label: {
                if busy { ProgressView().controlSize(.small).frame(minWidth: 120) } else { Text("Test and Save").frame(minWidth: 120) }
            }
            .buttonStyle(.borderedProminent)
            .controlSize(.large)
            .disabled(busy || baseURL.isEmpty || model.trimmingCharacters(in: .whitespaces).isEmpty)
            .keyboardShortcut(.defaultAction)
        }
        .onAppear {
            guard let s = app.settings, !s.baseURL.isEmpty else { return }
            baseURL = s.baseURL
            provider = s.provider
            model = s.model
            preset = EndpointPreset.all.first { $0.baseURL == s.baseURL && $0.provider == s.provider }
        }
    }

    private func choose(_ p: EndpointPreset) {
        preset = p
        provider = p.provider
        baseURL = p.baseURL
        model = ""
        status = nil
        // A local server needs no key, so its models can be listed now.
        if !p.needsKey || app.settings?.hasAPIKey == true || !apiKey.isEmpty {
            Task { await load() }
        }
    }

    private func load() async {
        await app.loadModels(baseURL: baseURL, apiKey: apiKey.isEmpty ? nil : apiKey)
    }

    private func connect() async {
        busy = true
        defer { busy = false }
        let id = model.trimmingCharacters(in: .whitespaces)
        do {
            let test = try await app.client.testConnection(
                provider: provider, baseURL: baseURL, model: id, apiKey: apiKey.isEmpty ? nil : apiKey)
            guard test.ok else {
                failed = true
                status = test.reason ?? "The model did not answer."
                return
            }
        } catch {
            failed = true
            status = error.localizedDescription
            return
        }
        var update = SettingsUpdate(provider: provider, baseURL: baseURL, model: id)
        if !apiKey.isEmpty { update.apiKey = apiKey }
        if let error = await app.save(update) {
            failed = true
            status = error
            return
        }
        failed = false
        status = "The model answered."
        apiKey = ""
        try? await Task.sleep(for: .milliseconds(450))
        next()
    }
}

private struct DonePage: View {
    @Bindable var app: AppState
    let finish: () -> Void
    @State private var burst = false

    var body: some View {
        VStack(spacing: 20) {
            Spacer()
            ZStack {
                ForEach(0..<12, id: \.self) { i in
                    Circle()
                        .fill(Color.accentColor.opacity(0.8))
                        .frame(width: 8, height: 8)
                        .offset(y: burst ? -88 : 0)
                        .rotationEffect(.degrees(Double(i) * 30))
                        .opacity(burst ? 0 : 1)
                }
                Circle()
                    .fill(Color.accentColor.gradient)
                    .frame(width: 110, height: 110)
                    .scaleEffect(burst ? 1 : 0.4)
                Image(systemName: "checkmark")
                    .font(.system(size: 48, weight: .bold))
                    .foregroundStyle(.white)
                    .symbolEffect(.bounce, value: burst)
            }
            .frame(width: 200, height: 200)
            VStack(spacing: 8) {
                Text("You're set")
                    .font(.system(size: 30, weight: .bold, design: .rounded))
                Text(summary)
                    .foregroundStyle(.secondary)
                    .multilineTextAlignment(.center)
                    .frame(maxWidth: 420)
            }
            Spacer()
            Button("Start a Conversation", action: finish)
                .buttonStyle(.borderedProminent)
                .controlSize(.large)
                .keyboardShortcut(.defaultAction)
                .padding(.bottom, 34)
        }
        .onAppear { withAnimation(.spring(response: 0.6, dampingFraction: 0.6).delay(0.1)) { burst = true } }
    }

    private var summary: String {
        let model = app.settings?.model ?? "your model"
        let folder = app.workspace?.path.map { ($0 as NSString).lastPathComponent } ?? "your folder"
        return "Messages go to \(model), and Zorp works inside \(folder). Change either any time in Settings."
    }
}

// MARK: pieces

/// Icon, title and subtitle at the top, a body, then Back and a primary
/// action at the bottom: the frame every middle step shares.
private struct PageScaffold<Content: View, Footer: View, Trigger: Equatable>: View {
    let symbol: String
    let symbolTrigger: Trigger
    let title: String
    let subtitle: String
    let back: () -> Void
    @ViewBuilder let content: Content
    @ViewBuilder let footer: Footer

    var body: some View {
        VStack(spacing: 18) {
            Image(systemName: symbol)
                .font(.system(size: 44, weight: .medium))
                .foregroundStyle(Color.accentColor.gradient)
                .symbolEffect(.bounce, value: symbolTrigger)
                .contentTransition(.symbolEffect(.replace))
                .padding(.top, 18)
            VStack(spacing: 6) {
                Text(title).font(.system(size: 26, weight: .bold, design: .rounded))
                Text(subtitle)
                    .foregroundStyle(.secondary)
                    .multilineTextAlignment(.center)
                    .frame(maxWidth: 460)
            }
            content
                .frame(maxWidth: 520)
            Spacer(minLength: 0)
            HStack {
                Button("Back", action: back).buttonStyle(.borderless)
                Spacer()
                footer
            }
            .padding(.bottom, 26)
        }
        .padding(.horizontal, 36)
    }
}

private struct PresetCard: View {
    let preset: EndpointPreset
    let selected: Bool
    let action: () -> Void
    @State private var hover = false

    var body: some View {
        Button(action: action) {
            HStack(spacing: 8) {
                Image(systemName: preset.needsKey ? "cloud" : "desktopcomputer")
                    .foregroundStyle(selected ? Color.white : Color.accentColor)
                Text(preset.name).lineLimit(1)
                Spacer(minLength: 0)
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 10)
            .foregroundStyle(selected ? Color.white : Color.primary)
            .background(selected ? AnyShapeStyle(Color.accentColor.gradient) : AnyShapeStyle(.background.opacity(hover ? 0.9 : 0.55)),
                        in: RoundedRectangle(cornerRadius: 10))
            .overlay(RoundedRectangle(cornerRadius: 10).stroke(Color.accentColor.opacity(selected ? 0 : (hover ? 0.6 : 0.15))))
            .scaleEffect(hover && !selected ? 1.03 : 1)
            .contentShape(RoundedRectangle(cornerRadius: 10))
        }
        .buttonStyle(.plain)
        .onHover { h in withAnimation(.easeOut(duration: 0.15)) { hover = h } }
    }
}

private struct StepDots: View {
    let step: OnboardingView.Step
    @Namespace private var ns

    var body: some View {
        HStack(spacing: 6) {
            ForEach(OnboardingView.Step.allCases, id: \.self) { s in
                ZStack {
                    Capsule().fill(Color.secondary.opacity(0.25)).frame(width: 8, height: 8)
                    if s == step {
                        Capsule().fill(Color.accentColor)
                            .frame(width: 24, height: 8)
                            .matchedGeometryEffect(id: "dot", in: ns)
                    }
                }
                .frame(width: s == step ? 24 : 8, height: 8)
            }
        }
        .animation(.spring(response: 0.45, dampingFraction: 0.8), value: step)
        .accessibilityElement()
        .accessibilityLabel("Step \(step.rawValue + 1) of \(OnboardingView.Step.allCases.count)")
    }
}

/// The app icon with two dots orbiting it, breathing slowly.
private struct OrbitingIcon: View {
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    var body: some View {
        TimelineView(.animation(paused: reduceMotion)) { ctx in
            let t = ctx.date.timeIntervalSinceReferenceDate
            ZStack {
                Circle().stroke(Color.accentColor.opacity(0.18), lineWidth: 1.5)
                Circle().stroke(Color.accentColor.opacity(0.10), lineWidth: 1).scaleEffect(0.78)
                Image(nsImage: NSApp.applicationIconImage)
                    .resizable()
                    .frame(width: 92, height: 92)
                    .scaleEffect(1 + 0.03 * sin(t * 1.6))
                    .shadow(color: Color.accentColor.opacity(0.35), radius: 16)
                orbiter(t * 0.9, radius: 75, size: 9)
                orbiter(-t * 1.4 + 2, radius: 58, size: 6)
            }
        }
    }

    private func orbiter(_ angle: Double, radius: CGFloat, size: CGFloat) -> some View {
        Circle()
            .fill(Color.accentColor)
            .frame(width: size, height: size)
            .offset(x: cos(angle) * radius, y: sin(angle) * radius)
    }
}

/// Soft colour fields drifting behind the card.
private struct AuroraBackground: View {
    let paused: Bool

    var body: some View {
        TimelineView(.animation(paused: paused)) { ctx in
            let t = ctx.date.timeIntervalSinceReferenceDate / 6
            Canvas { gc, size in
                gc.addFilter(.blur(radius: 70))
                let blobs: [(Color, Double, Double)] = [
                    (.accentColor, 0, 0.42), (.purple, 2.1, 0.36), (.teal, 4.2, 0.32),
                ]
                for (color, phase, scale) in blobs {
                    let d = min(size.width, size.height) * scale * 2
                    let x = size.width * (0.5 + 0.32 * cos(t + phase))
                    let y = size.height * (0.5 + 0.28 * sin(t * 1.3 + phase))
                    gc.fill(Path(ellipseIn: CGRect(x: x - d / 2, y: y - d / 2, width: d, height: d)),
                            with: .color(color.opacity(0.45)))
                }
            }
        }
        .background(Color(nsColor: .windowBackgroundColor))
        .ignoresSafeArea()
    }
}

private struct TrailingIconLabelStyle: LabelStyle {
    func makeBody(configuration: Configuration) -> some View {
        HStack(spacing: 6) {
            configuration.title
            configuration.icon
        }
    }
}
