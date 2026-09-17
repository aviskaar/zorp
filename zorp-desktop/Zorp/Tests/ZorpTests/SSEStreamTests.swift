#if canImport(Testing)
import Testing
@testable import ZorpKit
import Foundation

@Test func testParseAssistantDelta() {
    let json = #"{"type":"assistant_delta","text":"Hello world"}"#
    let event = SSEStream.parseEvent(data: json.data(using: .utf8)!, seq: 42)
    #expect(event != nil)
    #expect(event?.seq == 42)
    if case .assistantDelta(let text) = event?.kind {
        #expect(text == "Hello world")
    } else {
        Issue.record("Expected assistantDelta")
    }
}

@Test func testParseApprovalRequest() {
    let json = #"{"type":"approval_request","id":"app-1","tool":"bash","arguments":"{\"cmd\":\"ls\"}"}"#
    let event = SSEStream.parseEvent(data: json.data(using: .utf8)!, seq: 100)
    #expect(event != nil)
    if case .approvalRequest(let id, let tool, _) = event?.kind {
        #expect(id == "app-1")
        #expect(tool == "bash")
    } else {
        Issue.record("Expected approvalRequest")
    }
}

@Test func testParseCheckpointRequest() {
    let json = #"{"type":"checkpoint_request","id":"cp-1","kind":"investigate","prompt":"Threshold reached"}"#
    let event = SSEStream.parseEvent(data: json.data(using: .utf8)!, seq: 101)
    #expect(event != nil)
    if case .checkpointRequest(let id, let kind, let prompt) = event?.kind {
        #expect(id == "cp-1")
        #expect(kind == "investigate")
        #expect(prompt == "Threshold reached")
    } else {
        Issue.record("Expected checkpointRequest")
    }
}

@Test func testParseContext() {
    let json = #"{"type":"context","used_tokens":1234,"limit_tokens":8000,"source":"reported"}"#
    let event = SSEStream.parseEvent(data: json.data(using: .utf8)!, seq: 102)
    #expect(event != nil)
    if case .context(let used, let limit, let source) = event?.kind {
        #expect(used == 1234)
        #expect(limit == 8000)
        #expect(source == "reported")
    } else {
        Issue.record("Expected context")
    }
}
#endif
