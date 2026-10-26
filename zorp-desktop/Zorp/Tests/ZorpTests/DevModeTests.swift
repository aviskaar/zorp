#if canImport(Testing)
import Foundation
import Testing
@testable import ZorpKit

@Test func trainEventsParse() {
    let initial = TrainEvent.parse(Data(#"{"type":"init","parameters":250000000,"device":"gpu","memory_total_gb":36,"data":"corpus","corpus_tokens":1000,"dropped_tokens":3}"#.utf8))
    #expect(initial == .initial(parameters: 250_000_000, device: "gpu", memoryTotalGB: 36, data: "corpus", corpusTokens: 1000, droppedTokens: 3))
    // A run that said nothing about its data is "not reported", never synthetic.
    let silent = TrainEvent.parse(Data(#"{"type":"init","parameters":1,"device":"gpu","memory_total_gb":1}"#.utf8))
    if case .initial(_, _, _, let data, _, _)? = silent { #expect(data == nil) } else { Issue.record("expected init") }
    let step = TrainEvent.parse(Data(#"{"type":"step","step":10,"loss":3.5,"lr":0.0003,"tokens":4096,"tok_per_sec":900.5,"memory_gb":4.2,"eta_seconds":60}"#.utf8))
    #expect(step == .step(step: 10, loss: 3.5, lr: 0.0003, tokens: 4096, tokPerSec: 900.5, memoryGB: 4.2, etaSeconds: 60))
    #expect(TrainEvent.parse(Data(#"{"type":"error","message":"no MLX"}"#.utf8)) == .error(message: "no MLX"))
}

@Test func aTrainingConfigLeavesAbsentPathsOut() throws {
    let config = TrainingJobConfig(runId: "r", recipeName: "zorp-dense-250m")
    let object = try JSONSerialization.jsonObject(with: JSONEncoder().encode(config)) as? [String: Any]
    #expect(object?["recipe_name"] as? String == "zorp-dense-250m")
    #expect(object?["tokenizer_dir"] == nil)
    #expect(object?["dataset_path"] == nil)
    #expect(object?["sample_every_steps"] as? Int == 50)
}

@Test func aRecipeCountsItsParameters() {
    let recipe = ArchitectureRecipe(
        name: "tiny", family: "qwen", vocabSize: 100, maxPositionEmbeddings: 128, hiddenSize: 8,
        intermediateSize: 16, numHiddenLayers: 2, numAttentionHeads: 2, numKeyValueHeads: 1,
        rmsNormEps: 1e-6, ropeTheta: 10000, qkNorm: false, tieWordEmbeddings: true)
    let p = recipe.parameters
    #expect(p.embeddingParams == 800)
    #expect(p.totalParams == p.embeddingParams + p.attentionParams + p.mlpParams + p.normParams)
}

@MainActor
@Test func aCheckpointPathTravelsAsOneSegment() async throws {
    StubProtocol.reset(["POST /api/dev/models//tmp/runs/r1/step-100/serve": (200, #"{"status":"ok","port":9000,"base_url":"http://127.0.0.1:9000/v1"}"#)])
    let served = try await stubClient().devServe(checkpointDir: "/tmp/runs/r1/step-100")
    #expect(served.baseURL == "http://127.0.0.1:9000/v1")
}

@Test func aSegmentEncodesSlashes() {
    #expect(ZorpClient.segment("/tmp/a b") == "%2Ftmp%2Fa%20b")
    let url = stubClient().url(encodedPath: "/api/dev/models/\(ZorpClient.segment("/tmp/r"))/serve")
    #expect(url.absoluteString == "http://127.0.0.1:7777/api/dev/models/%2Ftmp%2Fr/serve")
}
#endif
