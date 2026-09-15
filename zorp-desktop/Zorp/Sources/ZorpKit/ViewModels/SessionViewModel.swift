import Foundation
import Observation

public struct ChatMessage: Identifiable, Hashable, Sendable {
    public let id: String
    public let role: String // "user" or "assistant"
    public var text: String
    public var isStreaming: Bool

    public init(id: String = UUID().uuidString, role: String, text: String, isStreaming: Bool = false) {
        self.id = id
        self.role = role
        self.text = text
        self.isStreaming = isStreaming
    }
}

public struct ToolCallItem: Identifiable, Hashable, Sendable {
    public let id: String
    public let name: String
    public var phrase: String?
    public var summary: String?
    public var isRunning: Bool

    public init(id: String = UUID().uuidString, name: String, phrase: String? = nil, summary: String? = nil, isRunning: Bool = true) {
        self.id = id
        self.name = name
        self.phrase = phrase
        self.summary = summary
        self.isRunning = isRunning
    }
}

public struct InvestigateProgressState: Sendable {
    public let phase: String
    public let attempt: Int?
    public let of: Int?
    public let ledger: LedgerFrame?

    public init(phase: String, attempt: Int? = nil, of: Int? = nil, ledger: LedgerFrame? = nil) {
        self.phase = phase
        self.attempt = attempt
        self.of = of
        self.ledger = ledger
    }
}

@Observable
public final class SessionViewModel: @unchecked Sendable {
    public let sessionId: String
    public let client: ZorpClient
    public let streamURL: URL

    public var messages: [ChatMessage] = []
    public var tools: [ToolCallItem] = []
    public var pendingApproval: (id: String, tool: String, arguments: String)?
    public var pendingCheckpoint: (id: String, kind: String, prompt: String)?
    public var investigateState: InvestigateProgressState?
    public var isWorking: Bool = false
    public var usedTokens: UInt64 = 0
    public var limitTokens: UInt64? = nil
    public var contextSource: String = "estimated"

    private var sseTask: Task<Void, Never>?
    private var streamingMessageId: String?

    public init(sessionId: String, client: ZorpClient, baseURL: URL) {
        self.sessionId = sessionId
        self.client = client
        self.streamURL = baseURL.appendingPathComponent("api/sessions/\(sessionId)/events")
    }

    public func connect() {
        let stream = SSEStream(url: streamURL)
        sseTask?.cancel()
        sseTask = Task { @MainActor in
            do {
                for try await event in stream.events() {
                    self.handleEvent(event)
                }
            } catch {
                print("SSE stream closed or error: \(error)")
            }
        }
    }

    public func disconnect() {
        sseTask?.cancel()
        sseTask = nil
    }

    public func send(prompt: String) async {
        let userMsg = ChatMessage(role: "user", text: prompt)
        messages.append(userMsg)
        do {
            try await client.startTurn(sessionId: sessionId, prompt: prompt)
        } catch {
            messages.append(ChatMessage(role: "assistant", text: "Error starting turn: \(error.localizedDescription)"))
        }
    }

    public func handleEvent(_ event: ServerEvent) {
        switch event.kind {
        case .working:
            isWorking = true
        case .workingDone:
            isWorking = false
        case .toolStarted(let name, let phrase):
            tools.append(ToolCallItem(name: name, phrase: phrase, summary: nil, isRunning: true))
        case .tool(let name, let summary, let phrase):
            if let idx = tools.lastIndex(where: { $0.name == name && $0.isRunning }) {
                tools[idx].summary = summary
                tools[idx].phrase = phrase ?? tools[idx].phrase
                tools[idx].isRunning = false
            } else {
                tools.append(ToolCallItem(name: name, phrase: phrase, summary: summary, isRunning: false))
            }
        case .assistantDelta(let text):
            if let id = streamingMessageId, let idx = messages.firstIndex(where: { $0.id == id }) {
                messages[idx].text.append(text)
            } else {
                let newId = UUID().uuidString
                streamingMessageId = newId
                messages.append(ChatMessage(id: newId, role: "assistant", text: text, isStreaming: true))
            }
        case .assistantWithdrawn:
            if let id = streamingMessageId {
                messages.removeAll(where: { $0.id == id })
                streamingMessageId = nil
            }
        case .assistant(let text):
            if let id = streamingMessageId, let idx = messages.firstIndex(where: { $0.id == id }) {
                messages[idx].text = text
                messages[idx].isStreaming = false
            } else {
                messages.append(ChatMessage(role: "assistant", text: text, isStreaming: false))
            }
            streamingMessageId = nil
        case .approvalRequest(let id, let tool, let arguments):
            pendingApproval = (id: id, tool: tool, arguments: arguments)
        case .checkpointRequest(let id, let kind, let prompt):
            pendingCheckpoint = (id: id, kind: kind, prompt: prompt)
        case .investigateProgress(let phase, let attempt, let of, let ledger):
            investigateState = InvestigateProgressState(phase: phase, attempt: attempt, of: of, ledger: ledger)
        case .context(let used, let limit, let source):
            usedTokens = used
            limitTokens = limit
            contextSource = source
        default:
            break
        }
    }
}
