#if canImport(Testing)
import Foundation
import Testing
@testable import ZorpKit

/// Answers requests from a table and remembers what was sent, so a test
/// can check the exact body a route received. This is the check that
/// would have caught the turn being posted as `prompt` instead of
/// `message`.
final class StubProtocol: URLProtocol, @unchecked Sendable {
    nonisolated(unsafe) static var routes: [String: (Int, String)] = [:]
    nonisolated(unsafe) static var sent: [(method: String, path: String, body: [String: Any])] = []
    nonisolated(unsafe) static var contentTypes: [String: String] = [:]
    static let lock = NSLock()

    /// Add routes. Suites run side by side, so this never clears what
    /// another test set up; each test uses session ids of its own.
    static func reset(_ table: [String: (Int, String)]) {
        lock.lock(); defer { lock.unlock() }
        routes.merge(table) { _, new in new }
    }

    static func bodies(for path: String) -> [[String: Any]] {
        lock.lock(); defer { lock.unlock() }
        return sent.filter { $0.path == path }.map(\.body)
    }

    override class func canInit(with request: URLRequest) -> Bool { true }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }

    override func startLoading() {
        let path = request.url?.path ?? ""
        let method = request.httpMethod ?? "GET"
        var body: [String: Any] = [:]
        if let data = request.httpBody ?? request.httpBodyStream.flatMap(Self.read),
           let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any] {
            body = object
        }
        Self.lock.lock()
        Self.sent.append((method, path, body))
        Self.contentTypes["\(method) \(path)"] = request.value(forHTTPHeaderField: "Content-Type")
        let (status, text) = Self.routes["\(method) \(path)"] ?? (404, "not found")
        Self.lock.unlock()
        let response = HTTPURLResponse(url: request.url!, statusCode: status, httpVersion: nil, headerFields: ["Content-Type": "application/json"])!
        client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
        client?.urlProtocol(self, didLoad: Data(text.utf8))
        client?.urlProtocolDidFinishLoading(self)
    }

    override func stopLoading() {}

    static func read(_ stream: InputStream) -> Data {
        stream.open(); defer { stream.close() }
        var data = Data()
        var buffer = [UInt8](repeating: 0, count: 4096)
        while stream.hasBytesAvailable {
            let n = stream.read(&buffer, maxLength: buffer.count)
            if n <= 0 { break }
            data.append(buffer, count: n)
        }
        return data
    }
}

func stubClient() -> ZorpClient {
    let config = URLSessionConfiguration.ephemeral
    config.protocolClasses = [StubProtocol.self]
    return ZorpClient(baseURL: URL(string: "http://127.0.0.1:7777")!, session: URLSession(configuration: config))
}

@MainActor
@Suite(.serialized) struct SessionViewModelTests {
    @Test func aTurnIsPostedAsMessage() async {
        StubProtocol.reset([
            "POST /api/sessions": (200, #"{"id":"s1"}"#),
            "POST /api/sessions/s1/turn": (202, ""),
        ])
        let vm = SessionViewModel(client: stubClient())
        await vm.send("hello there")
        vm.close()
        let bodies = StubProtocol.bodies(for: "/api/sessions/s1/turn")
        #expect(bodies.count == 1)
        #expect(bodies.first?["message"] as? String == "hello there")
        #expect(bodies.first?["memory"] as? Bool == false)
        #expect(bodies.first?["prompt"] == nil)
        #expect(vm.sessionId == "s1")
        #expect(vm.items.first?.kind == .user(text: "hello there"))
    }

    @Test func noWorkspaceSaysSoAndAsksForOne() async {
        StubProtocol.reset([
            "POST /api/sessions": (200, #"{"id":"s2"}"#),
            "POST /api/sessions/s2/turn": (409, "no workspace chosen"),
        ])
        final class Host: SessionHost {
            var asked = false
            func sessionCreated(_ id: String) {}
            func sessionsChanged() {}
            func needsWorkspace() { asked = true }
            func turnFinished() {}
        }
        let host = Host()
        let vm = SessionViewModel(client: stubClient())
        vm.host = host
        await vm.send("hi")
        vm.close()
        #expect(host.asked)
        #expect(!vm.isRunning)
        if case .error(let text) = vm.items.last?.kind {
            #expect(text.contains("workspace"))
        } else {
            Issue.record("expected an error row")
        }
    }

    @Test func approvalsPostAllow() async {
        StubProtocol.reset(["POST /api/sessions/s3/approve": (200, "")])
        let vm = SessionViewModel(client: stubClient(), sessionId: "s3")
        vm.apply(ServerEvent(seq: 1, kind: .approvalRequest(id: "r", tool: "run_command", arguments: "{}")))
        let id = vm.items[0].id
        await vm.answerApproval(id, allow: true)
        #expect(StubProtocol.bodies(for: "/api/sessions/s3/approve").first?["allow"] as? Bool == true)
        #expect(vm.items[0].kind == .approval(tool: "run_command", arguments: "{}", decision: true))
    }

    @Test func aStreamedAnswerSettlesAndCountsForBranching() {
        let vm = SessionViewModel(client: stubClient(), sessionId: "s4")
        vm.apply(ServerEvent(seq: 1, kind: .working))
        vm.apply(ServerEvent(seq: 2, kind: .assistantDelta(text: "Hel")))
        vm.apply(ServerEvent(seq: 3, kind: .assistantDelta(text: "lo")))
        #expect(vm.items.last?.kind == .assistant(text: "Hello", streaming: true, ordinal: nil))
        vm.apply(ServerEvent(seq: 4, kind: .assistant(text: "Hello")))
        #expect(vm.items.last?.kind == .assistant(text: "Hello", streaming: false, ordinal: 1))
        vm.apply(ServerEvent(seq: 5, kind: .done))
        #expect(!vm.isRunning)
    }

    @Test func aWithdrawnStreamIsTakenDown() {
        let vm = SessionViewModel(client: stubClient(), sessionId: "s5")
        vm.apply(ServerEvent(seq: 1, kind: .assistantDelta(text: "partial")))
        vm.apply(ServerEvent(seq: 2, kind: .assistantWithdrawn(events: 1, reask: 1, bound: 2)))
        #expect(vm.items.isEmpty)
    }

    @Test func aToolLineStartsRunningAndSettlesInPlace() {
        let vm = SessionViewModel(client: stubClient(), sessionId: "s6")
        vm.apply(ServerEvent(seq: 1, kind: .toolStarted(name: "run_command(ls)", phrase: nil)))
        vm.apply(ServerEvent(seq: 2, kind: .tool(name: "run_command(ls)", summary: "exited 0", phrase: nil)))
        #expect(vm.items.count == 1)
        #expect(vm.items[0].kind == .tool(name: "run_command(ls)", phrase: nil, summary: "exited 0", state: .ok))
    }

    @Test func aReplayedFrameIsDrawnOnce() {
        let vm = SessionViewModel(client: stubClient(), sessionId: "s7")
        vm.receive(ServerEvent(seq: 1, kind: .notice(text: "a")))
        vm.receive(ServerEvent(seq: 1, kind: .notice(text: "a")))
        #expect(vm.items.count == 1)
    }

    @Test func catchUpSkipsAFinishedTurnAndJoinsALiveOne() {
        let vm = SessionViewModel(client: stubClient(), sessionId: "s8")
        vm.connect(joining: true)
        vm.close()
        vm.receive(ServerEvent(seq: 1, kind: .assistant(text: "old")))
        vm.receive(ServerEvent(seq: 2, kind: .done))
        vm.receive(ServerEvent(seq: 3, kind: .toolStarted(name: "read_file", phrase: nil)))
        vm.applyCatchUp()
        #expect(vm.items.count == 1)
        #expect(vm.isRunning)
    }

    @Test func anErrorEndsTheTurnAndAnsweredCardsSettle() {
        let vm = SessionViewModel(client: stubClient(), sessionId: "s9")
        vm.apply(ServerEvent(seq: 1, kind: .working))
        vm.apply(ServerEvent(seq: 2, kind: .approvalRequest(id: "r", tool: "t", arguments: "a")))
        vm.apply(ServerEvent(seq: 3, kind: .error(message: "boom")))
        #expect(!vm.isRunning)
        #expect(vm.items[0].kind == .approval(tool: "t", arguments: "a", decision: false))
    }

    @Test func aMessageSentDuringATurnIsQueued() async {
        let vm = SessionViewModel(client: stubClient(), sessionId: "s10")
        vm.apply(ServerEvent(seq: 1, kind: .working))
        await vm.send("later")
        #expect(vm.queue == ["later"])
        #expect(vm.items.isEmpty)
    }
}

@Test func toolStatusColours() {
    #expect(ActivityLine.state(for: "exited 0") == .ok)
    #expect(ActivityLine.state(for: "exited 2") == .failed)
    #expect(ActivityLine.state(for: "denied") == .failed)
    #expect(ActivityLine.state(for: "error: no such file") == .failed)
    #expect(ActivityLine.state(for: "a.txt (12 lines)") == .ok)
    #expect(ActivityLine.state(for: "0/3 blocks applied") == .failed)
}

@Test func shellCommandsGetAPhrase() {
    #expect(ActivityLine.describe("ls -la src") == "Listing files in src")
    #expect(ActivityLine.describe("cd web && npm test") == "Running tests")
    #expect(ActivityLine.describe("git status | head") == "Checking git status …")
    #expect(ActivityLine.headline(name: "run_command(ls)", phrase: "  \"Look around\" ").text == "Look around")
    #expect(ActivityLine.split("read_file").command == nil)
}

@Test func markdownBlocksParse() {
    let blocks = MarkdownBlock.parse("# Title\n\nSome *text*.\n\n- a\n- b\n\n```swift\nlet x = 1\n```\n\n| a | b |\n|---|---|\n| 1 | 2 |")
    #expect(blocks == [
        .heading(level: 1, text: "Title"),
        .paragraph("Some *text*."),
        .bullet(items: ["a", "b"]),
        .code(language: "swift", text: "let x = 1"),
        .table(header: ["a", "b"], rows: [["1", "2"]]),
    ])
}
#endif
