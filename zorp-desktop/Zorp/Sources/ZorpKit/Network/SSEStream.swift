import Foundation

/// The state of a session's event stream, for the status line.
public enum StreamStatus: Equatable, Sendable {
    case connecting
    case open
    case reconnecting
    case closed
}

/// Reads `GET /api/sessions/:id/events` the way a browser's EventSource
/// does: one long response, an event per `data:` block, and on a dropped
/// connection a reconnect carrying `Last-Event-ID` so the server resumes
/// after the last frame seen. A 404 ends it for good, because the server
/// says that when the session does not exist and retrying would only
/// repeat the question.
public final class SSEStream: @unchecked Sendable {
    private let url: URL
    private let session: URLSession

    public init(url: URL) {
        self.url = url
        let config = URLSessionConfiguration.default
        // The stream is meant to stay open for hours. The server sends a
        // keep-alive comment well inside this, so this bounds a dead
        // socket, never a quiet session.
        config.timeoutIntervalForRequest = 120
        config.timeoutIntervalForResource = 7 * 24 * 3600
        config.requestCachePolicy = .reloadIgnoringLocalCacheData
        self.session = URLSession(configuration: config)
    }

    public func events(onStatus: @escaping @Sendable (StreamStatus) -> Void = { _ in }) -> AsyncStream<ServerEvent> {
        AsyncStream { continuation in
            let task = Task {
                var lastSeq: UInt64?
                var delay: UInt64 = 500_000_000
                onStatus(.connecting)
                while !Task.isCancelled {
                    var request = URLRequest(url: self.url)
                    request.setValue("text/event-stream", forHTTPHeaderField: "Accept")
                    if let lastSeq {
                        request.setValue(String(lastSeq), forHTTPHeaderField: "Last-Event-ID")
                    }
                    do {
                        let (bytes, response) = try await self.session.bytes(for: request)
                        if let http = response as? HTTPURLResponse, http.statusCode == 404 {
                            break
                        }
                        guard let http = response as? HTTPURLResponse, (200...299).contains(http.statusCode) else {
                            throw URLError(.badServerResponse)
                        }
                        onStatus(.open)
                        delay = 500_000_000
                        var data = ""
                        for try await line in bytes.lines {
                            if Task.isCancelled { break }
                            if let payload = SSEStream.dataField(line) {
                                data += data.isEmpty ? payload : "\n" + payload
                                // axum writes one JSON object per data line and
                                // `lines` drops the blank separator, so a
                                // complete object is a complete event.
                                if let event = ServerEvent.parse(Data(data.utf8)) {
                                    lastSeq = event.seq
                                    continuation.yield(event)
                                    data = ""
                                }
                            }
                        }
                    } catch {
                        if Task.isCancelled { break }
                    }
                    if Task.isCancelled { break }
                    onStatus(.reconnecting)
                    try? await Task.sleep(nanoseconds: delay)
                    delay = min(delay * 2, 8_000_000_000)
                }
                onStatus(.closed)
                continuation.finish()
            }
            continuation.onTermination = { _ in task.cancel() }
        }
    }

    /// The value of a `data:` line, or nil for any other line: an `id:`,
    /// an `event:`, a `:` keep-alive comment or a blank separator.
    static func dataField(_ line: String) -> String? {
        guard line.hasPrefix("data:") else { return nil }
        var value = line.dropFirst(5)
        if value.first == " " { value = value.dropFirst() }
        return String(value)
    }
}
