#if canImport(Testing)
import Testing
@testable import ZorpKit
import Foundation

@Test func testStreamingMessageAndWithdrawal() {
    let baseURL = URL(string: "http://127.0.0.1:7777")!
    let client = ZorpClient(baseURL: baseURL)
    let vm = SessionViewModel(sessionId: "test-1", client: client, baseURL: baseURL)

    vm.handleEvent(ServerEvent(seq: 1, kind: .assistantDelta(text: "Hello")))
    #expect(vm.messages.count == 1)
    #expect(vm.messages[0].text == "Hello")
    #expect(vm.messages[0].isStreaming == true)

    vm.handleEvent(ServerEvent(seq: 2, kind: .assistantDelta(text: " world")))
    #expect(vm.messages[0].text == "Hello world")

    // Test withdrawal rollback
    vm.handleEvent(ServerEvent(seq: 3, kind: .assistantWithdrawn(events: 2, reask: 1, bound: 3)))
    #expect(vm.messages.count == 0)
}

@Test func testFinalAssistantSettlement() {
    let baseURL = URL(string: "http://127.0.0.1:7777")!
    let client = ZorpClient(baseURL: baseURL)
    let vm = SessionViewModel(sessionId: "test-2", client: client, baseURL: baseURL)

    vm.handleEvent(ServerEvent(seq: 1, kind: .assistantDelta(text: "Part 1")))
    vm.handleEvent(ServerEvent(seq: 2, kind: .assistant(text: "Final complete text")))
    #expect(vm.messages.count == 1)
    #expect(vm.messages[0].text == "Final complete text")
    #expect(vm.messages[0].isStreaming == false)
}

@Test func testToolLifecycleEvents() {
    let baseURL = URL(string: "http://127.0.0.1:7777")!
    let client = ZorpClient(baseURL: baseURL)
    let vm = SessionViewModel(sessionId: "test-3", client: client, baseURL: baseURL)

    vm.handleEvent(ServerEvent(seq: 1, kind: .toolStarted(name: "read_file", phrase: "Reading config")))
    #expect(vm.tools.count == 1)
    #expect(vm.tools[0].name == "read_file")
    #expect(vm.tools[0].phrase == "Reading config")
    #expect(vm.tools[0].isRunning == true)

    vm.handleEvent(ServerEvent(seq: 2, kind: .tool(name: "read_file", summary: "File contents (42 bytes)", phrase: "Reading config")))
    #expect(vm.tools.count == 1)
    #expect(vm.tools[0].summary == "File contents (42 bytes)")
    #expect(vm.tools[0].isRunning == false)
}
#endif
