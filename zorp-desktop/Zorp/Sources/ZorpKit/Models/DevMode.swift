import Foundation

// Developer Mode: local pretraining on Apple Silicon through MLX, served by
// `zorp-web`'s `/api/dev/*` routes (the `train` feature). Each type mirrors
// its interface in `web/src/api.ts` and its struct in `zorp-train`.

public struct DevStatus: Codable, Sendable {
    /// `ready`, or a word saying what is missing.
    public let environmentStatus: String
    public let pythonPath: String?

    enum CodingKeys: String, CodingKey {
        case environmentStatus = "environment_status"
        case pythonPath = "python_path"
    }
}

/// What most dev routes answer: `ok`, or `error` with the reason.
public struct DevResult: Codable, Sendable {
    public let status: String
    public let error: String?

    public var ok: Bool { status == "ok" }
}

public struct ArchitectureRecipe: Codable, Hashable, Identifiable, Sendable {
    public let name: String
    public let family: String
    public let vocabSize: Int
    public let maxPositionEmbeddings: Int
    public let hiddenSize: Int
    public let intermediateSize: Int
    public let numHiddenLayers: Int
    public let numAttentionHeads: Int
    public let numKeyValueHeads: Int
    public let rmsNormEps: Double
    public let ropeTheta: Double
    public let qkNorm: Bool
    public let tieWordEmbeddings: Bool

    public var id: String { name }

    enum CodingKeys: String, CodingKey {
        case name, family
        case vocabSize = "vocab_size"
        case maxPositionEmbeddings = "max_position_embeddings"
        case hiddenSize = "hidden_size"
        case intermediateSize = "intermediate_size"
        case numHiddenLayers = "num_hidden_layers"
        case numAttentionHeads = "num_attention_heads"
        case numKeyValueHeads = "num_key_value_heads"
        case rmsNormEps = "rms_norm_eps"
        case ropeTheta = "rope_theta"
        case qkNorm = "qk_norm"
        case tieWordEmbeddings = "tie_word_embeddings"
    }

    /// Parameter count, the same arithmetic as the server's calculator:
    /// embeddings, attention with grouped keys and values, a gated MLP and
    /// the norms, with the output head shared when the embeddings are tied.
    public var parameters: ParameterBreakdown {
        let h = hiddenSize
        let headDim = h / max(numAttentionHeads, 1)
        let kv = numKeyValueHeads * headDim
        let embedding = vocabSize * h * (tieWordEmbeddings ? 1 : 2)
        let attention = numHiddenLayers * (h * h + 2 * h * kv + h * h)
        let mlp = numHiddenLayers * 3 * h * intermediateSize
        let norms = numHiddenLayers * 2 * h + h + (qkNorm ? numHiddenLayers * 2 * headDim : 0)
        return ParameterBreakdown(
            embeddingParams: embedding, attentionParams: attention, mlpParams: mlp,
            normParams: norms, totalParams: embedding + attention + mlp + norms)
    }
}

public struct ParameterBreakdown: Codable, Hashable, Sendable {
    public let embeddingParams: Int
    public let attentionParams: Int
    public let mlpParams: Int
    public let normParams: Int
    public let totalParams: Int

    enum CodingKeys: String, CodingKey {
        case embeddingParams = "embedding_params"
        case attentionParams = "attention_params"
        case mlpParams = "mlp_params"
        case normParams = "norm_params"
        case totalParams = "total_params"
    }
}

public struct DevRecipes: Codable, Sendable {
    public let recipes: [ArchitectureRecipe]
    public let defaultBreakdown: ParameterBreakdown?

    enum CodingKeys: String, CodingKey {
        case recipes
        case defaultBreakdown = "default_breakdown"
    }
}

public struct TokenInspection: Codable, Sendable {
    public let tokens: [String]
    public let ids: [Int]
    public let charCount: Int
    public let tokenCount: Int
    public let compressionCharsPerToken: Double

    enum CodingKeys: String, CodingKey {
        case tokens, ids
        case charCount = "char_count"
        case tokenCount = "token_count"
        case compressionCharsPerToken = "compression_chars_per_token"
    }
}

struct InspectResponse: Codable {
    let result: TokenInspection?
    let error: String?
}

/// A pretraining run's settings, as `zorp_train::TrainingJobConfig` reads
/// them. A path left nil is absent, not empty: the server then trains on
/// synthetic tokens and says so in the run's first event.
public struct TrainingJobConfig: Codable, Sendable {
    public var runId: String
    public var datasetId: String = "default"
    public var tokenizerName: String = "default"
    public var recipeName: String
    public var batchSize: Int = 4
    public var gradientAccumulationSteps: Int = 4
    public var learningRate: Double = 3e-4
    public var warmupSteps: Int = 50
    public var maxTokens: Int = 500_000_000
    public var checkpointEverySteps: Int = 100
    public var sampleEverySteps: Int = 50
    public var tokenizerDir: String?
    public var datasetPath: String?

    public init(runId: String, recipeName: String) {
        self.runId = runId
        self.recipeName = recipeName
    }

    enum CodingKeys: String, CodingKey {
        case runId = "run_id"
        case datasetId = "dataset_id"
        case tokenizerName = "tokenizer_name"
        case recipeName = "recipe_name"
        case batchSize = "batch_size"
        case gradientAccumulationSteps = "gradient_accumulation_steps"
        case learningRate = "learning_rate"
        case warmupSteps = "warmup_steps"
        case maxTokens = "max_tokens"
        case checkpointEverySteps = "checkpoint_every_steps"
        case sampleEverySteps = "sample_every_steps"
        case tokenizerDir = "tokenizer_dir"
        case datasetPath = "dataset_path"
    }

    public func encode(to encoder: Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        try c.encode(runId, forKey: .runId)
        try c.encode(datasetId, forKey: .datasetId)
        try c.encode(tokenizerName, forKey: .tokenizerName)
        try c.encode(recipeName, forKey: .recipeName)
        try c.encode(batchSize, forKey: .batchSize)
        try c.encode(gradientAccumulationSteps, forKey: .gradientAccumulationSteps)
        try c.encode(learningRate, forKey: .learningRate)
        try c.encode(warmupSteps, forKey: .warmupSteps)
        try c.encode(maxTokens, forKey: .maxTokens)
        try c.encode(checkpointEverySteps, forKey: .checkpointEverySteps)
        try c.encode(sampleEverySteps, forKey: .sampleEverySteps)
        try c.encodeIfPresent(tokenizerDir, forKey: .tokenizerDir)
        try c.encodeIfPresent(datasetPath, forKey: .datasetPath)
    }
}

public struct CheckpointMetadata: Codable, Hashable, Identifiable, Sendable {
    public let runId: String
    public let step: Int
    public let loss: Double
    public let checkpointDir: String
    public let createdAtIso: String

    public var id: String { checkpointDir }

    enum CodingKeys: String, CodingKey {
        case step, loss
        case runId = "run_id"
        case checkpointDir = "checkpoint_dir"
        case createdAtIso = "created_at_iso"
    }
}

struct DevModels: Codable {
    let models: [CheckpointMetadata]
}

public struct DevServe: Codable, Sendable {
    public let status: String
    public let port: Int?
    public let baseURL: String?
    public let error: String?

    enum CodingKeys: String, CodingKey {
        case status, port, error
        case baseURL = "base_url"
    }
}

/// One line of `GET /api/dev/train/stream`.
public enum TrainEvent: Equatable, Sendable {
    /// `data` is `corpus` or `synthetic` as the run reported it, and nil
    /// when it said nothing, which is "not reported" and never synthetic.
    case initial(parameters: Int, device: String, memoryTotalGB: Double, data: String?, corpusTokens: Int, droppedTokens: Int)
    case step(step: Int, loss: Double, lr: Double, tokens: Int, tokPerSec: Double, memoryGB: Double, etaSeconds: Int)
    case sample(step: Int, prompt: String, output: String)
    case checkpoint(step: Int, loss: Double, path: String)
    case error(message: String)

    public static func parse(_ data: Data) -> TrainEvent? {
        guard let o = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any],
              let type = o["type"] as? String else { return nil }
        func int(_ k: String) -> Int { (o[k] as? NSNumber)?.intValue ?? 0 }
        func dbl(_ k: String) -> Double { (o[k] as? NSNumber)?.doubleValue ?? 0 }
        func str(_ k: String) -> String { o[k] as? String ?? "" }
        switch type {
        case "init":
            return .initial(parameters: int("parameters"), device: str("device"), memoryTotalGB: dbl("memory_total_gb"),
                            data: o["data"] as? String, corpusTokens: int("corpus_tokens"), droppedTokens: int("dropped_tokens"))
        case "step":
            return .step(step: int("step"), loss: dbl("loss"), lr: dbl("lr"), tokens: int("tokens"),
                         tokPerSec: dbl("tok_per_sec"), memoryGB: dbl("memory_gb"), etaSeconds: int("eta_seconds"))
        case "sample":
            return .sample(step: int("step"), prompt: str("prompt"), output: str("output"))
        case "checkpoint":
            return .checkpoint(step: int("step"), loss: dbl("loss"), path: str("path"))
        case "error":
            return .error(message: str("message"))
        default:
            return nil
        }
    }
}

extension ZorpClient {
    public func devStatus() async throws -> DevStatus {
        try await json(DevStatus.self, "GET", "api/dev/status")
    }

    public func devSetupEnvironment() async throws -> DevResult {
        try await json(DevResult.self, "POST", "api/dev/environment/setup", body: encode([String: Any]()))
    }

    public func devRecipes() async throws -> DevRecipes {
        try await json(DevRecipes.self, "GET", "api/dev/recipes")
    }

    public func devTrainTokenizer(datasetPath: String, outputDir: String, vocabSize: Int?) async throws -> DevResult {
        var body: [String: Any] = ["dataset_path": datasetPath, "output_dir": outputDir]
        if let vocabSize {
            body["config"] = ["name": "default", "vocab_size": vocabSize]
        }
        return try await json(DevResult.self, "POST", "api/dev/tokenizer/train", body: encode(body))
    }

    public func devInspectTokens(tokenizerDir: String, text: String) async throws -> TokenInspection {
        let response = try await json(InspectResponse.self, "POST", "api/dev/tokenizer/inspect",
                                      body: encode(["tokenizer_dir": tokenizerDir, "text": text]))
        if let result = response.result { return result }
        throw APIError.http(status: 400, body: response.error ?? "the tokenizer could not be read")
    }

    public func devStartTraining(runDir: String, config: TrainingJobConfig) async throws -> DevResult {
        let configObject = try JSONSerialization.jsonObject(with: JSONEncoder().encode(config))
        return try await json(DevResult.self, "POST", "api/dev/train/start",
                              body: encode(["run_dir": runDir, "config": configObject]))
    }

    public func devPause() async throws -> DevResult {
        try await json(DevResult.self, "POST", "api/dev/train/pause", body: encode([String: Any]()))
    }

    public func devResume() async throws -> DevResult {
        try await json(DevResult.self, "POST", "api/dev/train/resume", body: encode([String: Any]()))
    }

    public func devStop() async throws -> DevResult {
        try await json(DevResult.self, "POST", "api/dev/train/stop", body: encode([String: Any]()))
    }

    public func devModels() async throws -> [CheckpointMetadata] {
        try await json(DevModels.self, "GET", "api/dev/models").models
    }

    public func devServe(checkpointDir: String) async throws -> DevServe {
        let path = "/api/dev/models/\(Self.segment(checkpointDir))/serve"
        let data = try await send("POST", path, body: encode([String: Any]()), encodedPath: path)
        do {
            return try JSONDecoder().decode(DevServe.self, from: data)
        } catch {
            throw APIError.decoding("\(path): \(error)")
        }
    }

    public var devStreamURL: URL { url("api/dev/train/stream") }
}
