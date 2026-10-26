import Foundation
import Observation

/// What a session tells the window around it.
@MainActor
public protocol SessionHost: AnyObject {
    /// A new chat got its id on the first send.
    func sessionCreated(_ id: String)
    /// The server named the conversation, or a turn finished.
    func sessionsChanged()
    /// A turn was refused because no workspace is chosen.
    func needsWorkspace()
    /// A turn finished, so the Files pane may have something new.
    func turnFinished()
    /// A run produced a file worth opening, such as an investigate draft.
    func openFile(_ path: String)
}

extension SessionHost {
    public func openFile(_ path: String) {}
}

/// One conversation: its timeline, its event stream and the controls that
/// act on it. Follows `web/src/main.ts`, which is the other client of the
/// same session, so both surfaces behave alike.
@MainActor
@Observable
public final class SessionViewModel {
    public private(set) var sessionId: String?
    public var title: String
    public private(set) var items: [TimelineItem] = []
    /// A turn, a panel or a run occupies the session.
    public private(set) var isRunning = false
    /// The model is between a request and its answer.
    public private(set) var isThinking = false
    public private(set) var streamStatus: StreamStatus = .closed
    public private(set) var usedTokens: Int64 = 0
    public private(set) var limitTokens: Int64?
    public private(set) var contextSource = "estimated"
    /// Tool approvals stood down for this conversation. Off by default and
    /// never carried into the next chat.
    public private(set) var autoApprove = false
    /// Messages typed while a turn was running, sent in order after it.
    public var queue: [String] = []
    public private(set) var agent: String?
    public private(set) var loadError: String?

    public let client: ZorpClient
    public weak var host: SessionHost?

    private var streamTask: Task<Void, Never>?
    private var streamingItem: UUID?
    private var compactingItem: UUID?
    private var panelItem: UUID?
    private var investigateItem: UUID?
    /// The question the running bolt is about, for reading its ledger.
    public private(set) var investigateQuestion: String?
    private var lastSeq: Int64 = -1
    private var answers = 0
    /// While joining a session, the server's replay is held here rather
    /// than drawn: it may describe a turn the stored transcript already
    /// shows. See `applyCatchUp`.
    private var catchUp: [ServerEvent]?
    static let catchUpWindow: UInt64 = 400_000_000

    public init(client: ZorpClient, sessionId: String? = nil, title: String = "New chat") {
        self.client = client
        self.sessionId = sessionId
        self.title = title
    }

    // MARK: opening

    /// Draw the stored transcript, then join the event stream.
    public func open() async {
        guard let id = sessionId else { return }
        do {
            let transcript = try await client.getSession(id: id)
            items = TimelineItem.replay(transcript)
            answers = items.reduce(0) { count, item in
                if case .assistant = item.kind { return count + 1 }
                return count
            }
            agent = transcript.agent
        } catch let error as APIError where error.status == 404 {
            // An in-memory session with nothing stored yet.
            items = []
        } catch {
            loadError = error.localizedDescription
            append(.error(text: "Could not load this conversation: \(error.localizedDescription)"))
        }
        if let on = try? await client.getAutoApprove(id: id) {
            autoApprove = on
        }
        connect(joining: true)
    }

    func connect(joining: Bool) {
        guard let id = sessionId, streamTask == nil else { return }
        lastSeq = -1
        catchUp = joining ? [] : nil
        let stream = SSEStream(url: client.eventsURL(id: id))
        streamTask = Task { @MainActor [weak self] in
            let items = stream.items()
            if joining {
                Task { @MainActor [weak self] in
                    try? await Task.sleep(nanoseconds: SessionViewModel.catchUpWindow)
                    self?.applyCatchUp()
                }
            }
            for await item in items {
                guard let self else { break }
                switch item {
                case .event(let event): self.receive(event)
                case .status(let status): self.streamStatus = status
                }
            }
        }
    }

    public func close() {
        streamTask?.cancel()
        streamTask = nil
        streamStatus = .closed
    }

    /// Everything up to and including the last `done` belongs to a turn
    /// that already finished, so the stored transcript has it. Anything
    /// after is a turn still in flight, worth joining.
    func applyCatchUp() {
        guard let buffered = catchUp else { return }
        catchUp = nil
        let start = (buffered.lastIndex { $0.type == "done" }).map { $0 + 1 } ?? 0
        guard start < buffered.count else { return }
        isRunning = true
        for event in buffered[start...] { apply(event) }
    }

    func receive(_ event: ServerEvent) {
        // A reconnect replays from the last id seen. Dropping anything at or
        // below the high-water mark keeps a generous replay from doubling
        // the timeline.
        if Int64(event.seq) <= lastSeq { return }
        lastSeq = Int64(event.seq)
        if catchUp != nil {
            catchUp!.append(event)
            return
        }
        apply(event)
    }

    // MARK: events

    func apply(_ event: ServerEvent) {
        switch event.kind {
        case .working:
            isRunning = true
            isThinking = true
        case .workingDone:
            isThinking = false
        case .toolStarted(let name, let phrase):
            append(.tool(name: name, phrase: phrase, summary: nil, state: .running))
        case .tool(let name, let summary, let phrase):
            let state = ActivityLine.state(for: summary)
            if let index = items.lastIndex(where: {
                if case .tool(let n, _, _, .running) = $0.kind { return n == name }
                return false
            }) {
                if case .tool(_, let old, _, _) = items[index].kind {
                    items[index].kind = .tool(name: name, phrase: phrase ?? old, summary: summary, state: state)
                }
            } else {
                append(.tool(name: name, phrase: phrase, summary: summary, state: state))
            }
        case .verify(let command, let passed):
            append(.verify(command: command, passed: passed))
        case .notice(let text):
            append(.notice(text: text))
        case .assistantDelta(let text):
            if let id = streamingItem, let index = items.firstIndex(where: { $0.id == id }),
               case .assistant(let sofar, _, _) = items[index].kind {
                items[index].kind = .assistant(text: sofar + text, streaming: true, ordinal: nil)
            } else {
                let item = TimelineItem(.assistant(text: text, streaming: true, ordinal: nil))
                streamingItem = item.id
                items.append(item)
            }
        case .assistantWithdrawn:
            if let id = streamingItem {
                items.removeAll { $0.id == id }
                streamingItem = nil
            }
        case .assistant(let text):
            answers += 1
            let kind = TimelineItem.Kind.assistant(text: text, streaming: false, ordinal: answers)
            if let id = streamingItem, let index = items.firstIndex(where: { $0.id == id }) {
                items[index].kind = kind
            } else if !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                append(kind)
            } else {
                answers -= 1
            }
            streamingItem = nil
        case .approvalRequest(_, let tool, let arguments):
            append(.approval(tool: tool, arguments: arguments, decision: nil))
        case .checkpointRequest(_, let kind, let prompt):
            append(.checkpoint(kind: kind, prompt: prompt, decision: nil))
        case .context(let used, let limit, let source):
            usedTokens = used
            limitTokens = limit
            contextSource = source
        case .compacting(let messages, _, _):
            let item = TimelineItem(.compacting(messages: messages))
            compactingItem = item.id
            items.append(item)
        case .compacted(let ok, let before, let after, let summary, let reason, _):
            let kind = TimelineItem.Kind.compaction(
                messages: 0, tokensBefore: before, tokensAfter: after, summary: summary, ok: ok, reason: reason)
            if let id = compactingItem, let index = items.firstIndex(where: { $0.id == id }) {
                if case .compacting(let messages) = items[index].kind {
                    items[index].kind = .compaction(
                        messages: messages, tokensBefore: before, tokensAfter: after,
                        summary: summary, ok: ok, reason: reason)
                } else {
                    items[index].kind = kind
                }
            } else {
                append(kind)
            }
            compactingItem = nil
        case .memory(let used, let unavailable):
            append(.memory(citations: used, unavailable: unavailable))
        case .sessionTitle(let title):
            self.title = title
            host?.sessionsChanged()
        case .error(let message):
            // The server sends `error` instead of `done`, not as well as it.
            append(.error(text: message))
            finishTurn()
        case .stopped:
            append(.stopped)
        case .done:
            finishTurn()
        case .reviewerStarted(let lens):
            updatePanel { $0.reviewers.append(PanelReviewer(lens: lens, state: .running, findings: [], answer: "", why: nil)) }
        case .reviewerFinished(let lens, let findings, let answer):
            updatePanel { run in
                if let i = run.reviewers.firstIndex(where: { $0.lens == lens }) {
                    run.reviewers[i].state = .finished
                    run.reviewers[i].findings = findings
                    run.reviewers[i].answer = answer
                } else {
                    run.reviewers.append(PanelReviewer(lens: lens, state: .finished, findings: findings, answer: answer, why: nil))
                }
            }
        case .reviewerFailed(let lens, let why):
            updatePanel { run in
                if let i = run.reviewers.firstIndex(where: { $0.lens == lens }) {
                    run.reviewers[i].state = .failed
                    run.reviewers[i].why = why
                } else {
                    run.reviewers.append(PanelReviewer(lens: lens, state: .failed, findings: [], answer: "", why: why))
                }
            }
        case .panelDone(let target, let requested, let verdicts, let complete, let agreements):
            updatePanel { run in
                run.target = target
                run.lensesRequested = requested
                run.verdicts = verdicts
                run.complete = complete
                run.agreements = agreements
            }
            panelItem = nil
        case .investigateProgress(_, let phase, let attempt, let of, let ledger):
            updateInvestigate { run in
                run.line = InvestigateRun.phaseLine(phase, attempt: attempt, of: of)
                run.attempt = attempt ?? run.attempt
                run.of = of ?? run.of
                if let ledger { run.ledger = ledger }
            }
        case .investigateDone(_, let approved, let needsPrereg, let artifact):
            updateInvestigate { run in
                run.verdict = InvestigateRun.verdictLine(approved)
                run.artifact = artifact
                run.needsPrereg = needsPrereg
                run.finished = true
            }
            if let artifact { host?.openFile(artifact) }
        case .unknown:
            break
        }
    }

    private func updatePanel(_ change: (inout PanelRun) -> Void) {
        if let id = panelItem, let index = items.firstIndex(where: { $0.id == id }),
           case .panel(var run) = items[index].kind {
            change(&run)
            items[index].kind = .panel(run)
        } else {
            var run = PanelRun()
            change(&run)
            let item = TimelineItem(.panel(run))
            panelItem = item.id
            items.append(item)
        }
    }

    private func updateInvestigate(_ change: (inout InvestigateRun) -> Void) {
        if let id = investigateItem, let index = items.firstIndex(where: { $0.id == id }),
           case .investigate(var run) = items[index].kind {
            change(&run)
            items[index].kind = .investigate(run)
        } else {
            var run = InvestigateRun()
            run.question = investigateQuestion
            change(&run)
            let item = TimelineItem(.investigate(run))
            investigateItem = item.id
            items.append(item)
        }
    }

    private func finishTurn() {
        panelItem = nil
        if let id = investigateItem, let index = items.firstIndex(where: { $0.id == id }),
           case .investigate(var run) = items[index].kind, !run.finished {
            run.finished = true
            run.verdict = run.verdict ?? InvestigateRun.verdictLine(nil)
            items[index].kind = .investigate(run)
        }
        investigateItem = nil
        isRunning = false
        isThinking = false
        streamingItem = nil
        // A pending card nobody answered is settled by the turn ending.
        for index in items.indices {
            switch items[index].kind {
            case .approval(let tool, let arguments, nil):
                items[index].kind = .approval(tool: tool, arguments: arguments, decision: false)
            case .checkpoint(let kind, let prompt, nil):
                items[index].kind = .checkpoint(kind: kind, prompt: prompt, decision: false)
            default:
                break
            }
        }
        host?.turnFinished()
        host?.sessionsChanged()
        if !queue.isEmpty {
            let next = queue.removeFirst()
            Task { await self.send(next) }
        }
    }

    func append(_ kind: TimelineItem.Kind) {
        items.append(TimelineItem(kind))
    }

    // MARK: actions

    /// Send a message. A message typed while a turn runs waits in the
    /// queue and goes when the turn ends.
    public func send(_ message: String, memory: Bool = false) async {
        let text = message.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !text.isEmpty else { return }
        if isRunning {
            queue.append(text)
            return
        }
        append(.user(text: text))
        isRunning = true
        do {
            if sessionId == nil {
                let id = try await client.createSession()
                sessionId = id
                if autoApprove {
                    autoApprove = (try? await client.setAutoApprove(id: id, on: true)) ?? false
                }
                host?.sessionCreated(id)
                connect(joining: false)
            }
            try await client.sendTurn(id: sessionId!, message: text, memory: memory)
        } catch let error as APIError {
            isRunning = false
            if error.isNoWorkspace {
                append(.error(text: "Choose a workspace folder before sending. Zorp works inside the folder you pick."))
                host?.needsWorkspace()
            } else if error.status == 409 {
                append(.error(text: "A turn is already running on this conversation. Wait for it to finish."))
            } else {
                append(.error(text: error.localizedDescription))
            }
        } catch {
            isRunning = false
            append(.error(text: error.localizedDescription))
        }
    }

    public func stop() async {
        guard let id = sessionId else { return }
        queue.removeAll()
        do {
            try await client.stopTurn(id: id)
        } catch let error as APIError where error.status == 409 {
            // Nothing was running; the page was behind.
            finishTurn()
        } catch {
            append(.error(text: error.localizedDescription))
        }
    }

    /// Answer the approval card at `itemId`.
    public func answerApproval(_ itemId: UUID, allow: Bool) async {
        guard let id = sessionId, let index = items.firstIndex(where: { $0.id == itemId }),
              case .approval(let tool, let arguments, nil) = items[index].kind else { return }
        items[index].kind = .approval(tool: tool, arguments: arguments, decision: allow)
        do {
            try await client.approve(id: id, allow: allow)
        } catch let error as APIError where error.status == 409 {
            // Nothing was waiting any more; the turn moved on.
        } catch {
            append(.error(text: error.localizedDescription))
        }
    }

    public func answerCheckpoint(_ itemId: UUID, allow: Bool) async {
        guard let id = sessionId, let index = items.firstIndex(where: { $0.id == itemId }),
              case .checkpoint(let kind, let prompt, nil) = items[index].kind else { return }
        items[index].kind = .checkpoint(kind: kind, prompt: prompt, decision: allow)
        do {
            try await client.resolveCheckpoint(id: id, allow: allow)
        } catch let error as APIError where error.status == 409 {
        } catch {
            append(.error(text: error.localizedDescription))
        }
    }

    public func setAutoApprove(_ on: Bool) async {
        guard let id = sessionId else {
            autoApprove = on
            return
        }
        do {
            autoApprove = try await client.setAutoApprove(id: id, on: on)
        } catch {
            append(.error(text: "Could not change approvals: \(error.localizedDescription)"))
        }
    }

    /// Copy this conversation up to its `ordinal`th answer into a new one.
    public func branch(at ordinal: Int) async -> String? {
        guard let id = sessionId else { return nil }
        do {
            return try await client.branchSession(id: id, answer: ordinal)
        } catch {
            append(.error(text: "Could not branch: \(error.localizedDescription)"))
            return nil
        }
    }

    public func compact(focus: String? = nil) async {
        guard let id = sessionId else { return }
        do {
            let result = try await client.compactSession(id: id, focus: focus)
            if !result.compacted {
                append(.notice(text: result.reason ?? "Nothing to compact yet."))
            }
        } catch {
            append(.error(text: "Could not compact: \(error.localizedDescription)"))
        }
    }

    /// Make sure this conversation exists on the server, for a panel or
    /// a run started before any message.
    private func ensureSession() async throws -> String {
        if let id = sessionId { return id }
        let id = try await client.createSession()
        sessionId = id
        host?.sessionCreated(id)
        connect(joining: false)
        return id
    }

    private func report(_ error: Error) {
        isRunning = false
        if let api = error as? APIError, api.isNoWorkspace {
            append(.error(text: "Choose a workspace folder first."))
            host?.needsWorkspace()
        } else if let api = error as? APIError, api.status == 501 {
            append(.error(text: api.localizedDescription))
        } else {
            append(.error(text: error.localizedDescription))
        }
    }

    /// Launch a review panel on `body`. A person launches it; nothing a
    /// model says can.
    public func startPanel(label: String, body: String, lenses: [String]) async {
        guard !isRunning else { return }
        isRunning = true
        do {
            let id = try await ensureSession()
            try await client.startPanel(id: id, label: label, body: body, lenses: lenses)
        } catch {
            report(error)
        }
    }

    /// Run the bolt on `question`: pre-registered attempts, then a write-up.
    public func startInvestigate(question: String, prereg: Preregistration?, askAtCheckpoints: Bool) async {
        guard !isRunning else { return }
        append(.user(text: question))
        investigateQuestion = question
        isRunning = true
        do {
            let id = try await ensureSession()
            try await client.startInvestigate(id: id, question: question, prereg: prereg, askAtCheckpoints: askAtCheckpoints)
        } catch {
            report(error)
        }
    }

    /// Let the running attempt finish and be written up, then stop. One
    /// way only.
    public func stopAfterAttempt() async {
        guard let id = sessionId else { return }
        do {
            try await client.stopAfterAttempt(id: id)
            append(.notice(text: "Winding down after this attempt. It will be recorded and written up."))
        } catch {
            append(.error(text: error.localizedDescription))
        }
    }

    /// The most recent answer, the default thing a panel reviews.
    public var lastAnswer: String? {
        for item in items.reversed() {
            if case .assistant(let text, false, _) = item.kind { return text }
        }
        return nil
    }

    public var contextFraction: Double? {
        guard let limit = limitTokens, limit > 0 else { return nil }
        return min(1, Double(usedTokens) / Double(limit))
    }
}
