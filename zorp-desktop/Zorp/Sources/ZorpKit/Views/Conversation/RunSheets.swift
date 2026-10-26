import SwiftUI

/// Launch a review panel. The lenses come from the server and are only
/// chosen here: a client that could write a reviewer's instructions could
/// tell one reviewer the answer it wanted.
struct PanelSheet: View {
    @Bindable var app: AppState
    var session: SessionViewModel
    @Binding var isPresented: Bool
    @State private var lenses: [PanelLens] = []
    @State private var chosen: Set<String> = []
    @State private var label = "the last answer"
    @State private var material = ""
    @State private var error: String?

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Review Panel").font(.title2)
            Text("Each reviewer reads the material from one angle, with read-only tools, and none sees what the others said. Agreement is counted afterwards.")
                .font(.callout).foregroundStyle(.secondary)
            TextField("What is under review", text: $label)
            TextEditor(text: $material)
                .font(.system(.callout, design: .monospaced))
                .frame(minHeight: 160)
                .overlay(RoundedRectangle(cornerRadius: 6).stroke(Color.secondary.opacity(0.25)))
            Text("Lenses").font(.headline)
            if lenses.isEmpty {
                ProgressView().controlSize(.small)
            }
            ScrollView {
                VStack(alignment: .leading, spacing: 6) {
                    ForEach(lenses) { lens in
                        Toggle(isOn: Binding(
                            get: { chosen.contains(lens.name) },
                            set: { on in if on { chosen.insert(lens.name) } else { chosen.remove(lens.name) } })
                        ) {
                            VStack(alignment: .leading, spacing: 1) {
                                Text(lens.name).bold()
                                Text(lens.instruction).font(.caption).foregroundStyle(.secondary).lineLimit(3)
                            }
                        }
                        .toggleStyle(.checkbox)
                    }
                }
            }
            .frame(maxHeight: 200)
            if let error { Text(error).foregroundStyle(.red).font(.caption) }
            HStack {
                Spacer()
                Button("Cancel") { isPresented = false }
                Button("Start Panel") {
                    let body = material.trimmingCharacters(in: .whitespacesAndNewlines)
                    guard !body.isEmpty else { error = "Give the panel something to read."; return }
                    isPresented = false
                    Task { await session.startPanel(label: label, body: body, lenses: Array(chosen)) }
                }
                .buttonStyle(.borderedProminent)
                .keyboardShortcut(.return, modifiers: [.command])
            }
        }
        .padding(20)
        .frame(width: 560, height: 620)
        .task {
            material = session.lastAnswer ?? ""
            if material.isEmpty { label = "" }
            do {
                lenses = try await app.client.lenses()
                chosen = Set(lenses.map(\.name))
            } catch {
                self.error = error.localizedDescription
            }
        }
    }
}

/// Run the bolt: attempts at a question against a pre-registered kill
/// threshold, then a write-up audited against what the attempts recorded.
struct InvestigateSheet: View {
    @Bindable var app: AppState
    var session: SessionViewModel
    @State var question: String
    @Binding var isPresented: Bool
    let onStart: () -> Void

    @State private var preregister = false
    @State private var metric = ""
    @State private var threshold = ""
    @State private var direction = "lower-is-better"
    @State private var askAtCheckpoints = false

    init(app: AppState, session: SessionViewModel, question: String, isPresented: Binding<Bool>, onStart: @escaping () -> Void) {
        self.app = app
        self.session = session
        _question = State(initialValue: question)
        _isPresented = isPresented
        self.onStart = onStart
    }

    private var prereg: Preregistration? {
        guard preregister, !metric.isEmpty, let value = Double(threshold) else { return nil }
        return Preregistration(metricName: metric, killThreshold: value, direction: direction)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Label("Investigate", systemImage: "bolt.fill").font(.title2)
            Text("Runs attempts at the question, each recorded with the conditions it ran under, then writes the track up. A track its own kill threshold rejects gets no write-up.")
                .font(.callout).foregroundStyle(.secondary)
            TextField("Question", text: $question, axis: .vertical)
                .lineLimit(2...5)
            Toggle("Pre-register a metric and kill threshold", isOn: $preregister)
            if preregister {
                Grid(alignment: .leading) {
                    GridRow {
                        Text("Metric")
                        TextField("e.g. p95_latency_ms", text: $metric)
                    }
                    GridRow {
                        Text("Kill threshold")
                        TextField("a number", text: $threshold)
                    }
                    GridRow {
                        Text("Direction")
                        Picker("", selection: $direction) {
                            Text("Lower is better").tag("lower-is-better")
                            Text("Higher is better").tag("higher-is-better")
                        }
                        .labelsHidden()
                    }
                }
                Text("This is committed before the first attempt and cannot be changed afterwards. Leave it off to use what is already on file for this question, or to let the model propose one.")
                    .font(.caption).foregroundStyle(.secondary)
            }
            Toggle("Ask me at each research checkpoint", isOn: $askAtCheckpoints)
            Text("Off approves checkpoints automatically. Rejecting one kills the track and is recorded.")
                .font(.caption).foregroundStyle(.secondary)
            if let forecasting = app.investigate?.forecasting {
                Text(forecasting
                    ? "Forecasting is on: each attempt records an expectation before it runs."
                    : "Forecasting is off, so attempts record no expectation. Set ZORP_FORECAST to turn it on.")
                    .font(.caption).foregroundStyle(.secondary)
            }
            Spacer()
            HStack {
                Spacer()
                Button("Cancel") { isPresented = false }
                Button("Start") {
                    let q = question.trimmingCharacters(in: .whitespacesAndNewlines)
                    guard !q.isEmpty else { return }
                    isPresented = false
                    onStart()
                    Task { await session.startInvestigate(question: q, prereg: prereg, askAtCheckpoints: askAtCheckpoints) }
                }
                .buttonStyle(.borderedProminent)
                .disabled(question.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || (preregister && prereg == nil))
                .keyboardShortcut(.return, modifiers: [.command])
            }
        }
        .padding(20)
        .frame(width: 520, height: 520)
    }
}
