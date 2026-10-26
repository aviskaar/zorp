#if canImport(Testing)
import Foundation
import Testing
@testable import ZorpKit

// The JSON here is what zorp-web sends, copied from its serializers:
// `event.rs` for frames and `api.rs` for route bodies. A field renamed on
// either side should break one of these.

private func event(_ json: String) -> ServerEvent? {
    ServerEvent.parse(Data(json.utf8))
}

@Test func aToolFrameCarriesItsSeqAndPhrase() {
    let e = event(#"{"seq":1,"type":"tool","name":"run_command(ls)","summary":"exited 0","phrase":"Listing files"}"#)
    #expect(e?.seq == 1)
    #expect(e?.kind == .tool(name: "run_command(ls)", summary: "exited 0", phrase: "Listing files"))
}

@Test func everyLifecycleFrameDecodes() {
    #expect(event(#"{"seq":2,"type":"working"}"#)?.kind == .working)
    #expect(event(#"{"seq":3,"type":"working_done"}"#)?.kind == .workingDone)
    #expect(event(#"{"seq":4,"type":"stopped"}"#)?.kind == .stopped)
    #expect(event(#"{"seq":5,"type":"done"}"#)?.kind == .done)
    #expect(event(#"{"seq":6,"type":"error","message":"model refused"}"#)?.kind == .error(message: "model refused"))
    #expect(event(#"{"seq":7,"type":"session_title","title":"Pricing study"}"#)?.kind == .sessionTitle(title: "Pricing study"))
    #expect(event(#"{"seq":8,"type":"assistant_withdrawn","events":3,"reask":1,"bound":2}"#)?.kind == .assistantWithdrawn(events: 3, reask: 1, bound: 2))
}

@Test func contextAndCompactionFramesDecode() {
    #expect(event(#"{"seq":9,"type":"context","used_tokens":1200,"limit_tokens":8000,"source":"reported"}"#)?.kind
        == .context(usedTokens: 1200, limitTokens: 8000, source: "reported"))
    #expect(event(#"{"seq":10,"type":"context","used_tokens":5,"source":"estimated"}"#)?.kind
        == .context(usedTokens: 5, limitTokens: nil, source: "estimated"))
    #expect(event(#"{"seq":11,"type":"compacted","ok":true,"tokens_before":900,"tokens_after":300,"summary":"s","manual":true}"#)?.kind
        == .compacted(ok: true, tokensBefore: 900, tokensAfter: 300, summary: "s", reason: nil, manual: true))
}

@Test func anUnknownTypeIsKeptNotDropped() {
    #expect(event(#"{"seq":12,"type":"from_the_future"}"#)?.kind == .unknown(type: "from_the_future"))
    #expect(event(#"{"seq":13}"#) == nil)
}

@Test func theSessionListDecodes() throws {
    let json = #"[{"id":"a","title":"First","status":"done","project_id":null,"agent":null,"updated":1700000000000},{"id":"b","title":"New chat","status":"running","project_id":null,"agent":null}]"#
    let rows = try JSONDecoder().decode([SessionSummary].self, from: Data(json.utf8))
    #expect(rows.count == 2)
    #expect(rows[0].isStored)
    #expect(!rows[1].isStored)
}

@Test func aTranscriptReplaysWithToolsAndMarkers() throws {
    let json = #"{"messages":[{"role":"user","content":"hi"},{"role":"tool","name":"run_command(ls)","summary":"exited 1"},{"role":"assistant","content":"done"}],"compactions":[{"id":1,"boundary_seq":0,"after":1,"messages":1,"summary":"earlier","tokens_before":10,"tokens_after":4,"created":0,"manual":false}],"agent":null}"#
    let transcript = try JSONDecoder().decode(Transcript.self, from: Data(json.utf8))
    let items = TimelineItem.replay(transcript)
    #expect(items.count == 4)
    #expect(items[0].kind == .user(text: "hi"))
    if case .compaction = items[1].kind {} else { Issue.record("marker should follow the first entry") }
    #expect(items[2].kind == .tool(name: "run_command(ls)", phrase: nil, summary: "exited 1", state: .failed))
    #expect(items[3].kind == .assistant(text: "done", streaming: false, ordinal: 1))
}

@Test func settingsAndModelsDecode() throws {
    let settings = #"{"provider":"openai","provider_source":"ui","base_url":"https://openrouter.ai/api/v1","base_url_source":"ui","model":"x/y","model_source":"ui","max_tokens":null,"max_tokens_source":"default","has_api_key":true,"api_key_source":"ui","configured":true}"#
    let s = try JSONDecoder().decode(Settings.self, from: Data(settings.utf8))
    #expect(s.model == "x/y")
    #expect(s.hasAPIKey)
    let models = #"{"models":["a","b"],"details":[{"id":"a","prompt_price":0,"completion_price":0},{"id":"b","output_modalities":["image"]}],"error":null}"#
    let list = try JSONDecoder().decode(ModelsList.self, from: Data(models.utf8))
    #expect(list.entries[0].isFree)
    #expect(!list.entries[1].answersWithText)
    let bare = try JSONDecoder().decode(ModelsList.self, from: Data(#"{"models":["only"],"error":null}"#.utf8))
    #expect(bare.entries.map(\.id) == ["only"])
}

@Test func workspaceAndProjectsDecode() throws {
    let ws = try JSONDecoder().decode(Workspace.self, from: Data(#"{"path":null,"scratch":null,"source":"none","configured":false}"#.utf8))
    #expect(!ws.configured)
    let projects = try JSONDecoder().decode(ProjectList.self, from: Data(#"{"projects":[{"id":"p","name":"Q3","created":1}]}"#.utf8))
    #expect(projects.projects.first?.name == "Q3")
}

@Test func aSettingsUpdateSendsOnlyWhatChanged() throws {
    let data = try JSONEncoder().encode(SettingsUpdate(model: "m"))
    let object = try JSONSerialization.jsonObject(with: data) as? [String: Any]
    #expect(object?.keys.sorted() == ["model"])
}

@Test func theNoWorkspaceRefusalIsRecognised() {
    #expect(APIError.http(status: 409, body: "no workspace chosen").isNoWorkspace)
    #expect(!APIError.http(status: 409, body: "a turn is already running").isNoWorkspace)
    #expect(APIError.http(status: 409, body: "a turn is already running").errorDescription == "a turn is already running")
}

@Test func sseDataLinesAreRead() {
    #expect(SSEStream.dataField(#"data: {"seq":1}"#) == #"{"seq":1}"#)
    #expect(SSEStream.dataField("id: 1") == nil)
    #expect(SSEStream.dataField(":keep-alive") == nil)
}
#endif
