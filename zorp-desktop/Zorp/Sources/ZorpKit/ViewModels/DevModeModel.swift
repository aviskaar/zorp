import Foundation
import Observation

/// Developer Mode's state: the training environment, the recipes, the
/// tokenizer, a running pretraining job and the checkpoints it left.
@MainActor
@Observable
public final class DevModeModel {
    public struct LossPoint: Identifiable, Equatable, Sendable {
        public let step: Int
        public let loss: Double
        public var id: Int { step }
    }

    public struct RunInfo: Equatable, Sendable {
        public var parameters = 0
        public var device = ""
        public var memoryTotalGB = 0.0
        /// `corpus` or `synthetic` as the run said, or nil when it said
        /// nothing. Nil reads "Not reported", never synthetic.
        public var data: String?
        public var corpusTokens = 0
        public var droppedTokens = 0
    }

    public let client: ZorpClient
    public private(set) var status: DevStatus?
    public private(set) var unavailable: String?
    public private(set) var recipes: [ArchitectureRecipe] = []
    public private(set) var checkpoints: [CheckpointMetadata] = []
    public private(set) var settingUp = false
    public var message: String?

    public private(set) var training = false
    public private(set) var paused = false
    public private(set) var info: RunInfo?
    public private(set) var losses: [LossPoint] = []
    public private(set) var lastStep: TrainEvent?
    public private(set) var samples: [(step: Int, prompt: String, output: String)] = []
    public private(set) var log: [String] = []

    private var streamTask: Task<Void, Never>?

    public init(client: ZorpClient) {
        self.client = client
    }

    public func load() async {
        do {
            status = try await client.devStatus()
            unavailable = nil
        } catch let error as APIError {
            unavailable = error.localizedDescription
            return
        } catch {
            unavailable = error.localizedDescription
            return
        }
        recipes = (try? await client.devRecipes().recipes) ?? []
        checkpoints = (try? await client.devModels()) ?? []
        listen()
    }

    public func setUpEnvironment() async {
        settingUp = true
        defer { settingUp = false }
        do {
            let result = try await client.devSetupEnvironment()
            message = result.ok ? "The training environment is ready." : (result.error ?? "Setup failed.")
            status = try? await client.devStatus()
        } catch {
            message = error.localizedDescription
        }
    }

    public func trainTokenizer(dataset: String, output: String, vocabSize: Int?) async {
        do {
            let result = try await client.devTrainTokenizer(datasetPath: dataset, outputDir: output, vocabSize: vocabSize)
            message = result.ok ? "Tokenizer written to \(output)." : (result.error ?? "Tokenizer training failed.")
        } catch {
            message = error.localizedDescription
        }
    }

    public func inspect(tokenizer: String, text: String) async -> TokenInspection? {
        do {
            return try await client.devInspectTokens(tokenizerDir: tokenizer, text: text)
        } catch {
            message = error.localizedDescription
            return nil
        }
    }

    public func start(config: TrainingJobConfig, runDir: String) async {
        losses = []
        samples = []
        info = nil
        lastStep = nil
        do {
            let result = try await client.devStartTraining(runDir: runDir, config: config)
            if result.ok {
                training = true
                paused = false
                message = "Run \(config.runId) started."
                listen()
            } else {
                message = result.error ?? "The run did not start."
            }
        } catch {
            message = error.localizedDescription
        }
    }

    public func pause() async {
        if let r = try? await client.devPause(), r.ok { paused = true } else { message = "Could not pause." }
    }

    public func resume() async {
        if let r = try? await client.devResume(), r.ok { paused = false } else { message = "Could not resume." }
    }

    public func stop() async {
        _ = try? await client.devStop()
        training = false
        paused = false
        checkpoints = (try? await client.devModels()) ?? checkpoints
    }

    /// Serve a checkpoint on loopback and point the next message at it.
    public func open(_ checkpoint: CheckpointMetadata, in app: AppState) async {
        do {
            let served = try await client.devServe(checkpointDir: checkpoint.checkpointDir)
            guard served.status == "ok", let base = served.baseURL else {
                message = served.error ?? "The checkpoint could not be served."
                return
            }
            let name = "\(checkpoint.runId)-step\(checkpoint.step)"
            if let error = await app.save(SettingsUpdate(provider: .openai, baseURL: base, model: name)) {
                message = error
            } else {
                message = "Serving \(name). New messages go to it."
                await app.loadModels(baseURL: base)
            }
        } catch {
            message = error.localizedDescription
        }
    }

    func apply(_ event: TrainEvent) {
        switch event {
        case .initial(let parameters, let device, let memory, let data, let corpus, let dropped):
            training = true
            info = RunInfo(parameters: parameters, device: device, memoryTotalGB: memory,
                           data: data, corpusTokens: corpus, droppedTokens: dropped)
        case .step(let step, let loss, _, _, _, _, _):
            training = true
            losses.append(LossPoint(step: step, loss: loss))
            if losses.count > 5000 { losses.removeFirst(losses.count - 5000) }
            lastStep = event
        case .sample(let step, let prompt, let output):
            samples.insert((step, prompt, output), at: 0)
            if samples.count > 20 { samples.removeLast() }
        case .checkpoint(let step, let loss, let path):
            log.insert("Checkpoint at step \(step), loss \(String(format: "%.4f", loss)): \(path)", at: 0)
            Task { checkpoints = (try? await client.devModels()) ?? checkpoints }
        case .error(let message):
            training = false
            log.insert("Error: \(message)", at: 0)
            self.message = message
        }
    }

    private func listen() {
        guard streamTask == nil else { return }
        let url = client.devStreamURL
        streamTask = Task { [weak self] in
            let config = URLSessionConfiguration.default
            config.timeoutIntervalForRequest = 120
            config.timeoutIntervalForResource = 7 * 24 * 3600
            let session = URLSession(configuration: config)
            while !Task.isCancelled {
                do {
                    let (bytes, _) = try await session.bytes(from: url)
                    for try await line in bytes.lines {
                        guard let payload = SSEStream.dataField(line),
                              let event = TrainEvent.parse(Data(payload.utf8)) else { continue }
                        self?.apply(event)
                    }
                } catch {
                    if Task.isCancelled { break }
                }
                try? await Task.sleep(nanoseconds: 2_000_000_000)
            }
        }
    }

    public func close() {
        streamTask?.cancel()
        streamTask = nil
    }
}
