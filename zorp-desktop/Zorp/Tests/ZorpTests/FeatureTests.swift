#if canImport(Testing)
import Foundation
import Testing
@testable import ZorpKit

@Test func featureListingsDecode() throws {
    let skills = try JSONDecoder().decode(SkillListing.self, from: Data(#"{"skills":[{"name":"landing-page","description":"d","path":"/p","scope":"workspace","declared_tools":[]}],"warnings":[],"offer":{"model":"qwen2.5:7b","tier":"plain","setting":"auto","reason":"small model","withheld":["react-components"]}}"#.utf8))
    #expect(skills.skills.first?.name == "landing-page")
    #expect(skills.offer?.withheld == ["react-components"])

    let agents = try JSONDecoder().decode(AgentListing.self, from: Data(#"{"agents":[{"name":"reviewer","scope":"workspace","description":null,"model":null,"tools":null,"approval_preset":null,"wants_privilege":true,"privilege_summary":["base URL"],"trusted":false,"fully_applied":false,"broken":null}],"default":"default"}"#.utf8))
    #expect(agents.agents.first?.wantsPrivilege == true)
    #expect(agents.defaultName == "default")

    let voice = try JSONDecoder().decode(VoiceStatus.self, from: Data(#"{"available":false,"runtime_reachable":false,"model_present":false,"setup_available":true,"endpoint":null,"model":null,"stage":null,"detail":"not set up"}"#.utf8))
    #expect(voice.setupAvailable)

    let recall = try JSONDecoder().decode(RecallStatus.self, from: Data(#"{"available":true,"reason":null,"endpoint":"http://127.0.0.1:11434","model":"m","conversations":3,"chunks":9,"memory":true}"#.utf8))
    #expect(recall.memory)

    let doctor = try JSONDecoder().decode(DoctorReport.self, from: Data(#"{"version":"0.6.0","healthy":false,"base_url":"x","checks":[{"health":"bad","label":"API key","detail":"not set"}]}"#.utf8))
    #expect(doctor.checks.first?.health == "bad")

    let data = try JSONDecoder().decode(DataState.self, from: Data(#"{"files":[{"label":"Conversations","what":"w","path":"/s","env_var":"ZORP_STORE","exists":true,"bytes":10}],"reset_note":"n"}"#.utf8))
    #expect(data.files.first?.envVar == "ZORP_STORE")
}

@MainActor
@Suite(.serialized) struct FeatureRequestTests {
    @Test func aBoltRunPostsTheQuestionAndPrereg() async {
        StubProtocol.reset(["POST /api/sessions/b1/investigate": (202, "")])
        let vm = SessionViewModel(client: stubClient(), sessionId: "b1")
        await vm.startInvestigate(
            question: "Does caching cut p95?",
            prereg: Preregistration(metricName: "p95_ms", killThreshold: 120, direction: "lower-is-better"),
            askAtCheckpoints: true)
        let body = StubProtocol.bodies(for: "/api/sessions/b1/investigate").first
        #expect(body?["question"] as? String == "Does caching cut p95?")
        #expect(body?["metric_name"] as? String == "p95_ms")
        #expect(body?["kill_threshold"] as? Double == 120)
        #expect(body?["threshold_direction"] as? String == "lower-is-better")
        #expect(body?["interactive_checkpoints"] as? Bool == true)
        #expect(body?["attempts"] == nil)
    }

    @Test func aBoltRunWithoutPreregSendsNulls() async {
        StubProtocol.reset(["POST /api/sessions/b2/investigate": (202, "")])
        let vm = SessionViewModel(client: stubClient(), sessionId: "b2")
        await vm.startInvestigate(question: "q", prereg: nil, askAtCheckpoints: false)
        let body = StubProtocol.bodies(for: "/api/sessions/b2/investigate").first
        #expect(body?["metric_name"] is NSNull)
        #expect(body?["interactive_checkpoints"] as? Bool == false)
    }

    @Test func aPanelPostsLabelBodyAndLenses() async {
        StubProtocol.reset(["POST /api/sessions/p1/panel": (202, "")])
        let vm = SessionViewModel(client: stubClient(), sessionId: "p1")
        await vm.startPanel(label: "draft", body: "text", lenses: ["skeptic"])
        let body = StubProtocol.bodies(for: "/api/sessions/p1/panel").first
        #expect(body?["label"] as? String == "draft")
        #expect(body?["body"] as? String == "text")
        #expect(body?["lenses"] as? [String] == ["skeptic"])
    }

    @Test func aBuildWithoutTheFeatureSaysSo() async {
        StubProtocol.reset(["POST /api/sessions/n1/investigate": (501, "this server was built without the research feature")])
        let vm = SessionViewModel(client: stubClient(), sessionId: "n1")
        await vm.startInvestigate(question: "q", prereg: nil, askAtCheckpoints: false)
        #expect(!vm.isRunning)
        #expect(vm.items.last?.kind == .error(text: "this server was built without the research feature"))
    }

    @Test func choosingAnAgentPutsItsName() async throws {
        StubProtocol.reset(["PUT /api/sessions/a1/agent": (200, "")])
        try await stubClient().setSessionAgent(id: "a1", agent: "reviewer")
        #expect(StubProtocol.bodies(for: "/api/sessions/a1/agent").first?["agent"] as? String == "reviewer")
    }

    @Test func panelFramesBuildOneBlock() {
        let vm = SessionViewModel(client: stubClient(), sessionId: "p2")
        vm.apply(ServerEvent(seq: 1, kind: .reviewerStarted(lens: "skeptic")))
        vm.apply(ServerEvent(seq: 2, kind: .reviewerStarted(lens: "editor")))
        vm.apply(ServerEvent(seq: 3, kind: .reviewerFinished(lens: "skeptic", findings: [PanelFindingFrame(severity: "concern", claim: "c", locus: "l")], answer: "a")))
        vm.apply(ServerEvent(seq: 4, kind: .reviewerFailed(lens: "editor", why: "timed out")))
        vm.apply(ServerEvent(seq: 5, kind: .panelDone(target: "draft", lensesRequested: 2, verdicts: 1, complete: false, agreements: [])))
        #expect(vm.items.count == 1)
        guard case .panel(let run) = vm.items[0].kind else { Issue.record("expected a panel"); return }
        #expect(run.reviewers.map(\.state) == [.finished, .failed])
        #expect(run.complete == false)
    }

    @Test func investigateFramesSayWhereTheRunIs() {
        let vm = SessionViewModel(client: stubClient(), sessionId: "i1")
        vm.apply(ServerEvent(seq: 1, kind: .investigateProgress(trackId: "t", phase: "attempt-started", attempt: 2, of: 3, ledger: nil)))
        guard case .investigate(let run) = vm.items.last?.kind else { Issue.record("expected a run"); return }
        #expect(run.line == "Attempt 2 of 3.")
        vm.apply(ServerEvent(seq: 2, kind: .investigateDone(trackId: "t", approved: false, needsPrereg: false, artifact: nil)))
        guard case .investigate(let done) = vm.items.last?.kind else { Issue.record("expected a run"); return }
        #expect(done.finished)
        #expect(done.verdict?.contains("killed") == true)
        #expect(vm.items.count == 1)
    }
}
#endif
