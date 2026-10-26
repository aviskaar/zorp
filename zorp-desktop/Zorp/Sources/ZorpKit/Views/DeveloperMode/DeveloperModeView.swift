import AppKit
import Charts
import SwiftUI
import UniformTypeIdentifiers

/// Developer Mode: train a tokenizer and a small model on this Mac with
/// MLX, watch the run, and chat with what it produced. Five tabs, the same
/// five the browser has.
public struct DeveloperModeView: View {
    @Bindable var app: AppState
    @State private var model: DevModeModel
    @State private var tab = "pretrain"

    public init(app: AppState) {
        self.app = app
        _model = State(initialValue: DevModeModel(client: app.client))
    }

    public var body: some View {
        Group {
            if let unavailable = model.unavailable {
                ContentUnavailableView("Developer Mode is unavailable", systemImage: "hammer",
                                       description: Text(unavailable))
            } else {
                TabView(selection: $tab) {
                    EnvironmentTab(model: model).tabItem { Label("Environment", systemImage: "cpu") }.tag("environment")
                    TokenizerTab(model: model).tabItem { Label("Tokenizer", systemImage: "textformat.abc") }.tag("tokenizer")
                    ArchitectureTab(model: model).tabItem { Label("Architecture", systemImage: "square.stack.3d.up") }.tag("architecture")
                    PretrainTab(model: model).tabItem { Label("Pretrain", systemImage: "chart.xyaxis.line") }.tag("pretrain")
                    RegistryTab(app: app, model: model).tabItem { Label("Models", systemImage: "shippingbox") }.tag("registry")
                }
                .padding()
            }
        }
        .frame(minWidth: 760, minHeight: 560)
        .safeAreaInset(edge: .bottom) {
            if let message = model.message {
                HStack {
                    Text(message).font(.callout).textSelection(.enabled)
                    Spacer()
                    Button { model.message = nil } label: { Image(systemName: "xmark") }.buttonStyle(.borderless)
                }
                .padding(10)
                .background(.bar)
            }
        }
        .task { await model.load() }
        .onDisappear { model.close() }
    }
}

struct EnvironmentTab: View {
    var model: DevModeModel
    @State private var datasetPath = ""
    @State private var preview: String?

    var body: some View {
        Form {
            Section("Training environment") {
                LabeledContent("Status", value: model.status?.environmentStatus ?? "checking")
                if let python = model.status?.pythonPath { LabeledContent("Python", value: python) }
                Text("Runs on this Mac's GPU through MLX, in its own Python environment at ~/.zorp/training-env.")
                    .font(.caption).foregroundStyle(.secondary)
                HStack {
                    Spacer()
                    Button(model.settingUp ? "Setting up…" : "Set Up Environment") {
                        Task { await model.setUpEnvironment() }
                    }
                    .disabled(model.settingUp || model.status?.environmentStatus == "ready")
                }
            }
            Section("Dataset") {
                HStack {
                    TextField("JSONL file of documents", text: $datasetPath)
                    Button("Choose…") {
                        if let path = FilePicker.file(types: ["jsonl", "json", "txt"]) { datasetPath = path; read() }
                    }
                }
                if let preview {
                    ScrollView {
                        Text(preview).font(.system(.caption, design: .monospaced)).textSelection(.enabled)
                            .frame(maxWidth: .infinity, alignment: .leading)
                    }
                    .frame(height: 180)
                }
            }
        }
        .formStyle(.grouped)
    }

    /// The first lines of the file, read from disk here because the file
    /// is on this Mac. Nothing is uploaded.
    private func read() {
        guard let handle = FileHandle(forReadingAtPath: datasetPath) else {
            preview = "Could not open the file."
            return
        }
        defer { try? handle.close() }
        let data = handle.readData(ofLength: 8000)
        let lines = String(decoding: data, as: UTF8.self).split(separator: "\n", omittingEmptySubsequences: true).prefix(8)
        preview = lines.joined(separator: "\n")
    }
}

struct TokenizerTab: View {
    var model: DevModeModel
    @State private var dataset = ""
    @State private var output = ""
    @State private var vocab = "32000"
    @State private var sample = "The quick brown fox jumps over the lazy dog."
    @State private var inspection: TokenInspection?
    @State private var busy = false

    var body: some View {
        Form {
            Section("Train a BPE tokenizer") {
                HStack {
                    TextField("Dataset (JSONL)", text: $dataset)
                    Button("Choose…") { if let p = FilePicker.file(types: ["jsonl", "json", "txt"]) { dataset = p } }
                }
                HStack {
                    TextField("Output folder", text: $output)
                    Button("Choose…") { if let p = FilePicker.folder() { output = p } }
                }
                TextField("Vocabulary size", text: $vocab)
                HStack {
                    Spacer()
                    Button(busy ? "Training…" : "Train Tokenizer") {
                        busy = true
                        Task {
                            await model.trainTokenizer(dataset: dataset, output: output, vocabSize: Int(vocab))
                            busy = false
                        }
                    }
                    .disabled(busy || dataset.isEmpty || output.isEmpty)
                }
            }
            Section("Inspect") {
                TextField("Text", text: $sample, axis: .vertical).lineLimit(2...4)
                HStack {
                    Spacer()
                    Button("Tokenize") {
                        Task { inspection = await model.inspect(tokenizer: output, text: sample) }
                    }
                    .disabled(output.isEmpty)
                }
                if let inspection {
                    Text("\(inspection.tokenCount) tokens for \(inspection.charCount) characters, \(String(format: "%.2f", inspection.compressionCharsPerToken)) characters per token")
                        .font(.caption)
                    FlowTokens(tokens: inspection.tokens)
                }
            }
        }
        .formStyle(.grouped)
    }
}

struct FlowTokens: View {
    let tokens: [String]

    var body: some View {
        ScrollView {
            LazyVGrid(columns: [GridItem(.adaptive(minimum: 40), spacing: 4)], alignment: .leading, spacing: 4) {
                ForEach(Array(tokens.enumerated()), id: \.offset) { index, token in
                    Text(token.replacingOccurrences(of: " ", with: "␣"))
                        .font(.system(.caption, design: .monospaced))
                        .padding(.horizontal, 4).padding(.vertical, 2)
                        .background(Color.accentColor.opacity(index.isMultiple(of: 2) ? 0.12 : 0.24))
                        .clipShape(RoundedRectangle(cornerRadius: 4))
                }
            }
        }
        .frame(maxHeight: 160)
    }
}

struct ArchitectureTab: View {
    var model: DevModeModel
    @State private var selected: String?

    var body: some View {
        HSplitView {
            List(model.recipes, selection: $selected) { recipe in
                VStack(alignment: .leading) {
                    Text(recipe.name)
                    Text("\(Self.count(recipe.parameters.totalParams)) parameters").font(.caption).foregroundStyle(.secondary)
                }
                .tag(recipe.name)
            }
            .frame(minWidth: 200)
            if let recipe = model.recipes.first(where: { $0.name == selected }) ?? model.recipes.first {
                Form {
                    Section(recipe.name) {
                        LabeledContent("Family", value: recipe.family)
                        LabeledContent("Layers", value: "\(recipe.numHiddenLayers)")
                        LabeledContent("Hidden size", value: "\(recipe.hiddenSize)")
                        LabeledContent("MLP size", value: "\(recipe.intermediateSize)")
                        LabeledContent("Attention heads", value: "\(recipe.numAttentionHeads) (\(recipe.numKeyValueHeads) key/value)")
                        LabeledContent("Vocabulary", value: "\(recipe.vocabSize)")
                        LabeledContent("Context", value: "\(recipe.maxPositionEmbeddings) tokens")
                        LabeledContent("Tied embeddings", value: recipe.tieWordEmbeddings ? "yes" : "no")
                    }
                    Section("Parameters") {
                        let p = recipe.parameters
                        Chart {
                            BarMark(x: .value("Count", p.embeddingParams), y: .value("Part", "Embeddings"))
                            BarMark(x: .value("Count", p.attentionParams), y: .value("Part", "Attention"))
                            BarMark(x: .value("Count", p.mlpParams), y: .value("Part", "MLP"))
                            BarMark(x: .value("Count", p.normParams), y: .value("Part", "Norms"))
                        }
                        .frame(height: 140)
                        LabeledContent("Total", value: Self.count(p.totalParams))
                    }
                }
                .formStyle(.grouped)
            } else {
                Text("No recipes.").foregroundStyle(.secondary)
            }
        }
    }

    static func count(_ n: Int) -> String {
        let d = Double(n)
        if d >= 1e9 { return String(format: "%.2fB", d / 1e9) }
        if d >= 1e6 { return String(format: "%.1fM", d / 1e6) }
        return "\(n)"
    }
}

struct PretrainTab: View {
    var model: DevModeModel
    @State private var recipe = ""
    @State private var tokenizerDir = ""
    @State private var datasetPath = ""
    @State private var batch = "4"
    @State private var accumulation = "4"
    @State private var learningRate = "0.0003"
    @State private var maxTokens = "500000000"

    var body: some View {
        HStack(alignment: .top, spacing: 16) {
            Form {
                Section("Run") {
                    Picker("Recipe", selection: $recipe) {
                        ForEach(model.recipes) { Text($0.name).tag($0.name) }
                    }
                    HStack {
                        TextField("Tokenizer folder", text: $tokenizerDir)
                        Button("…") { if let p = FilePicker.folder() { tokenizerDir = p } }
                    }
                    HStack {
                        TextField("Dataset (JSONL)", text: $datasetPath)
                        Button("…") { if let p = FilePicker.file(types: ["jsonl", "json", "txt"]) { datasetPath = p } }
                    }
                    if tokenizerDir.isEmpty || datasetPath.isEmpty {
                        Text("Without both a tokenizer and a dataset the run trains on synthetic tokens, and says so.")
                            .font(.caption).foregroundStyle(.orange)
                    }
                    TextField("Batch size", text: $batch)
                    TextField("Gradient accumulation", text: $accumulation)
                    TextField("Learning rate", text: $learningRate)
                    TextField("Max tokens", text: $maxTokens)
                }
                Section {
                    HStack {
                        Button("Start") { Task { await start() } }
                            .buttonStyle(.borderedProminent)
                            .disabled(model.training || recipe.isEmpty)
                        if model.training {
                            Button(model.paused ? "Resume" : "Pause") {
                                Task {
                                    if model.paused { await model.resume() } else { await model.pause() }
                                }
                            }
                            Button("Stop", role: .destructive) { Task { await model.stop() } }
                        }
                    }
                }
            }
            .formStyle(.grouped)
            .frame(width: 320)

            VStack(alignment: .leading, spacing: 10) {
                if let info = model.info {
                    HStack(spacing: 18) {
                        Metric(label: "Parameters", value: ArchitectureTab.count(info.parameters))
                        Metric(label: "Device", value: info.device)
                        Metric(label: "Training data", value: dataLine(info))
                    }
                }
                if case .step(let step, let loss, let lr, _, let tps, let mem, let eta)? = model.lastStep {
                    HStack(spacing: 18) {
                        Metric(label: "Step", value: "\(step)")
                        Metric(label: "Loss", value: String(format: "%.4f", loss))
                        Metric(label: "LR", value: String(format: "%.2e", lr))
                        Metric(label: "Tokens/s", value: String(format: "%.0f", tps))
                        Metric(label: "Memory", value: String(format: "%.1f GB", mem))
                        Metric(label: "ETA", value: Duration.seconds(eta).formatted(.units(allowed: [.hours, .minutes], width: .narrow)))
                    }
                }
                Chart(model.losses) { point in
                    LineMark(x: .value("Step", point.step), y: .value("Loss", point.loss))
                }
                .chartYAxisLabel("Loss")
                .chartXAxisLabel("Step")
                .frame(minHeight: 200)
                .overlay {
                    if model.losses.isEmpty {
                        Text(model.training ? "Waiting for the first step…" : "No run yet.").foregroundStyle(.secondary)
                    }
                }
                if !model.samples.isEmpty {
                    Text("Samples (model output)").font(.headline)
                    ScrollView {
                        VStack(alignment: .leading, spacing: 6) {
                            ForEach(Array(model.samples.enumerated()), id: \.offset) { _, sample in
                                VStack(alignment: .leading, spacing: 2) {
                                    Text("Step \(sample.step): \(sample.prompt)").font(.caption).foregroundStyle(.secondary)
                                    Text(sample.output).font(.system(.callout, design: .monospaced)).textSelection(.enabled)
                                }
                            }
                        }
                    }
                    .frame(maxHeight: 140)
                }
                if !model.log.isEmpty {
                    ScrollView {
                        VStack(alignment: .leading) {
                            ForEach(Array(model.log.enumerated()), id: \.offset) { _, line in
                                Text(line).font(.caption).textSelection(.enabled)
                            }
                        }
                    }
                    .frame(maxHeight: 80)
                }
            }
        }
        .onAppear {
            if recipe.isEmpty { recipe = model.recipes.first?.name ?? "" }
        }
        .onChange(of: model.recipes) {
            if recipe.isEmpty { recipe = model.recipes.first?.name ?? "" }
        }
    }

    private func dataLine(_ info: DevModeModel.RunInfo) -> String {
        switch info.data {
        case "corpus"?: return "\(info.corpusTokens) corpus tokens" + (info.droppedTokens > 0 ? ", \(info.droppedTokens) dropped" : "")
        case "synthetic"?: return "Synthetic tokens"
        case let other?: return other
        case nil: return "Not reported"
        }
    }

    private func start() async {
        var config = TrainingJobConfig(runId: "run-\(Int(Date().timeIntervalSince1970))", recipeName: recipe)
        config.batchSize = Int(batch) ?? 4
        config.gradientAccumulationSteps = Int(accumulation) ?? 4
        config.learningRate = Double(learningRate) ?? 3e-4
        config.maxTokens = Int(maxTokens) ?? 500_000_000
        config.tokenizerDir = tokenizerDir.isEmpty ? nil : tokenizerDir
        config.datasetPath = datasetPath.isEmpty ? nil : datasetPath
        await model.start(config: config, runDir: ".zorp/training/run")
    }
}

struct Metric: View {
    let label: String
    let value: String

    var body: some View {
        VStack(alignment: .leading, spacing: 1) {
            Text(label).font(.caption2).foregroundStyle(.secondary)
            Text(value).font(.callout).monospacedDigit()
        }
    }
}

struct RegistryTab: View {
    @Bindable var app: AppState
    var model: DevModeModel

    var body: some View {
        VStack(alignment: .leading) {
            Text("Checkpoints from local runs. Open in Zorp serves one on this Mac and points new messages at it.")
                .font(.callout).foregroundStyle(.secondary)
            List(model.checkpoints) { checkpoint in
                HStack {
                    VStack(alignment: .leading, spacing: 2) {
                        Text("\(checkpoint.runId), step \(checkpoint.step)")
                        Text("loss \(String(format: "%.4f", checkpoint.loss)) · \(checkpoint.createdAtIso)")
                            .font(.caption).foregroundStyle(.secondary)
                    }
                    Spacer()
                    Button("Show in Finder") {
                        NSWorkspace.shared.activateFileViewerSelecting([URL(fileURLWithPath: checkpoint.checkpointDir)])
                    }
                    Button("Open in Zorp") { Task { await model.open(checkpoint, in: app) } }
                        .buttonStyle(.borderedProminent)
                }
            }
            .overlay {
                if model.checkpoints.isEmpty { Text("No checkpoints yet.").foregroundStyle(.secondary) }
            }
        }
    }
}

/// Native open panels for a file or a folder.
@MainActor
enum FilePicker {
    static func file(types: [String]) -> String? {
        let panel = NSOpenPanel()
        panel.canChooseFiles = true
        panel.canChooseDirectories = false
        panel.allowsMultipleSelection = false
        panel.allowedContentTypes = types.compactMap { UTType(filenameExtension: $0) }
        return panel.runModal() == .OK ? panel.url?.path : nil
    }

    static func folder() -> String? {
        let panel = NSOpenPanel()
        panel.canChooseFiles = false
        panel.canChooseDirectories = true
        panel.canCreateDirectories = true
        panel.allowsMultipleSelection = false
        return panel.runModal() == .OK ? panel.url?.path : nil
    }
}
