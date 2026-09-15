import Foundation

public final class SSEStream: @unchecked Sendable {
    private let url: URL
    private let session: URLSession

    public init(url: URL, session: URLSession = .shared) {
        self.url = url
        self.session = session
    }

    public func events(startingFrom lastSeq: UInt64? = nil) -> AsyncThrowingStream<ServerEvent, Error> {
        AsyncThrowingStream { continuation in
            let task = Task {
                var request = URLRequest(url: self.url)
                request.setValue("text/event-stream", forHTTPHeaderField: "Accept")
                if let lastSeq = lastSeq {
                    request.setValue("\(lastSeq)", forHTTPHeaderField: "Last-Event-ID")
                }

                do {
                    let (asyncBytes, response) = try await self.session.bytes(for: request)
                    guard let httpResponse = response as? HTTPURLResponse,
                          (200...299).contains(httpResponse.statusCode) else {
                        throw URLError(.badServerResponse)
                    }

                    var currentSeq: UInt64 = lastSeq ?? 0
                    for try await line in asyncBytes.lines {
                        if Task.isCancelled { break }
                        let trimmed = line.trimmingCharacters(in: .whitespaces)
                        if trimmed.hasPrefix("id:") {
                            let idStr = trimmed.dropFirst(3).trimmingCharacters(in: .whitespaces)
                            if let parsed = UInt64(idStr) {
                                currentSeq = parsed
                            }
                        } else if trimmed.hasPrefix("data:") {
                            let jsonStr = trimmed.dropFirst(5).trimmingCharacters(in: .whitespaces)
                            if let data = jsonStr.data(using: .utf8),
                               let event = SSEStream.parseEvent(data: data, seq: currentSeq) {
                                continuation.yield(event)
                            }
                        }
                    }
                    continuation.finish()
                } catch {
                    continuation.finish(throwing: error)
                }
            }

            continuation.onTermination = { _ in
                task.cancel()
            }
        }
    }

    public static func parseEvent(data: Data, seq: UInt64) -> ServerEvent? {
        guard let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let type = json["type"] as? String else {
            return nil
        }

        let kind: ServerEventKind
        switch type {
        case "working":
            kind = .working
        case "working_done":
            kind = .workingDone
        case "tool":
            let name = json["name"] as? String ?? ""
            let summary = json["summary"] as? String ?? ""
            let phrase = json["phrase"] as? String
            kind = .tool(name: name, summary: summary, phrase: phrase)
        case "tool_started":
            let name = json["name"] as? String ?? ""
            let phrase = json["phrase"] as? String
            kind = .toolStarted(name: name, phrase: phrase)
        case "verify":
            let cmd = json["command"] as? String ?? ""
            let passed = json["passed"] as? Bool ?? false
            kind = .verify(command: cmd, passed: passed)
        case "notice":
            let text = json["text"] as? String ?? ""
            kind = .notice(text: text)
        case "assistant_delta":
            let text = json["text"] as? String ?? ""
            kind = .assistantDelta(text: text)
        case "assistant_withdrawn":
            let events = json["events"] as? Int ?? 0
            let reask = json["reask"] as? Int ?? 0
            let bound = json["bound"] as? Int ?? 0
            kind = .assistantWithdrawn(events: events, reask: reask, bound: bound)
        case "assistant":
            let text = json["text"] as? String ?? ""
            kind = .assistant(text: text)
        case "approval_request":
            let id = json["id"] as? String ?? ""
            let tool = json["tool"] as? String ?? ""
            let arguments = json["arguments"] as? String ?? ""
            kind = .approvalRequest(id: id, tool: tool, arguments: arguments)
        case "checkpoint_request":
            let id = json["id"] as? String ?? ""
            let cKind = json["kind"] as? String ?? ""
            let prompt = json["prompt"] as? String ?? ""
            kind = .checkpointRequest(id: id, kind: cKind, prompt: prompt)
        case "investigate_progress":
            let phase = json["phase"] as? String ?? ""
            let attempt = json["attempt"] as? Int
            let of = json["of"] as? Int
            var ledger: LedgerFrame? = nil
            if let lDict = json["ledger"] as? [String: Any],
               let lData = try? JSONSerialization.data(withJSONObject: lDict) {
                ledger = try? JSONDecoder().decode(LedgerFrame.self, from: lData)
            }
            kind = .investigateProgress(phase: phase, attempt: attempt, of: of, ledger: ledger)
        case "investigate_done":
            let trackId = json["track_id"] as? String ?? ""
            let approved = json["approved"] as? Bool
            let needsPrereg = json["needs_prereg"] as? Bool ?? false
            let artifact = json["artifact"] as? String
            kind = .investigateDone(trackId: trackId, approved: approved, needsPrereg: needsPrereg, artifact: artifact)
        case "context":
            let used = (json["used_tokens"] as? NSNumber)?.uint64Value ?? 0
            let limit = (json["limit_tokens"] as? NSNumber)?.uint64Value
            let source = json["source"] as? String ?? "estimated"
            kind = .context(usedTokens: used, limitTokens: limit, source: source)
        default:
            kind = .unknown(type: type)
        }

        return ServerEvent(seq: seq, kind: kind)
    }
}
