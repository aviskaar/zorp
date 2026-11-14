import AVFoundation
import Foundation
import Observation

/// Voice input: record from the microphone, get the local speech model
/// ready, and hand back a transcript for the composer.
///
/// The transcript is live. While the microphone is open the audio since the
/// last pause is sent again about once a second, and each answer replaces
/// the previous guess, so the words appear as they are spoken and get
/// corrected as more context arrives. At a pause, or when a stretch gets
/// long, the current guess is kept and the next stretch starts empty, so no
/// request ever carries more than `LiveWindow.maxSamples` of audio.
///
/// The recording goes to the in-process server and from there to the
/// Qwen3-ASR runtime on loopback, and nowhere else. A transcript is
/// editable composer text and is never sent on its own.
@MainActor
@Observable
public final class VoiceRecorder {
    public enum Phase: Equatable {
        case idle
        /// The runtime is being set up: an environment, a package, a model.
        case preparing(String)
        case listening
        case transcribing
    }

    public private(set) var phase: Phase = .idle
    /// 0...1, from the microphone stream. Amplitude only.
    public private(set) var level: Float = 0
    public private(set) var startedAt: Date?
    public private(set) var error: String?
    /// What has been heard so far, updated while listening. Model output
    /// from a microphone: shown as plain text, never sent on its own.
    public private(set) var liveText: String = ""

    private var engine: AVAudioEngine?
    private let buffer = SampleBuffer()
    private var transcript = LiveTranscript()
    private var loop: Task<Void, Never>?
    private var meterTimer: Timer?

    public init() {}

    public var isListening: Bool { phase == .listening }

    /// Start listening, preparing the runtime first if it is not ready.
    public func start(client: ZorpClient) async {
        guard phase == .idle else { return }
        error = nil
        guard await Self.microphoneAllowed() else {
            error = "Zorp is not allowed to use the microphone. Turn it on in System Settings, Privacy & Security, Microphone."
            return
        }
        do {
            let status = try await client.voiceStatus()
            guard status.available else {
                error = status.detail
                return
            }
            if !status.ready {
                guard status.setupAvailable else {
                    error = status.detail
                    return
                }
                phase = .preparing("Getting voice input ready. The first time takes a few minutes.")
                try await client.waitForVoice { event in
                    Task { @MainActor [weak self] in
                        self?.phase = .preparing(Self.stageLine(event.stage, detail: event.detail))
                    }
                }
            }
            try begin()
            loop = Task { [weak self] in await self?.transcribeWhileListening(client: client) }
        } catch {
            phase = .idle
            self.error = error.localizedDescription
        }
    }

    /// Stop and finish the transcript. The text comes back; nothing is sent.
    public func stop(client: ZorpClient) async -> String? {
        guard phase == .listening else {
            phase = .idle
            return nil
        }
        endCapture()
        phase = .transcribing
        // Let a request already in flight land before the last one, so two
        // answers for the same audio never race to the transcript.
        await loop?.value
        loop = nil
        defer { reset() }
        let rest = buffer.samples(from: transcript.windowStart)
        if rest.count >= LiveWindow.minSamples {
            do {
                let result = try await client.transcribe(audio: Wav.encode(rest), mimeType: "audio/wav")
                transcript.commit(result.text, through: buffer.count)
            } catch {
                self.error = "Could not transcribe: \(error.localizedDescription)"
                transcript.commitGuess(through: buffer.count)
            }
        } else {
            transcript.commitGuess(through: buffer.count)
        }
        let text = transcript.text
        return text.isEmpty ? nil : text
    }

    public func cancel() {
        endCapture()
        loop?.cancel()
        loop = nil
        reset()
    }

    private func reset() {
        phase = .idle
        level = 0
        startedAt = nil
        liveText = ""
        transcript = LiveTranscript()
        buffer.clear()
    }

    private func endCapture() {
        engine?.inputNode.removeTap(onBus: 0)
        engine?.stop()
        engine = nil
        meterTimer?.invalidate()
        meterTimer = nil
        level = 0
        startedAt = nil
    }

    private func begin() throws {
        buffer.clear()
        transcript = LiveTranscript()
        liveText = ""
        let engine = AVAudioEngine()
        let input = engine.inputNode
        let native = input.outputFormat(forBus: 0)
        guard let target = AVAudioFormat(commonFormat: .pcmFormatInt16, sampleRate: Double(LiveWindow.sampleRate), channels: 1, interleaved: true),
              let converter = AVAudioConverter(from: native, to: target)
        else {
            throw APIError.transport("the microphone format is not supported")
        }
        let sink = buffer
        input.installTap(onBus: 0, bufferSize: 4096, format: native) { pcm, _ in
            let ratio = target.sampleRate / native.sampleRate
            let capacity = AVAudioFrameCount(Double(pcm.frameLength) * ratio) + 16
            guard let out = AVAudioPCMBuffer(pcmFormat: target, frameCapacity: capacity) else { return }
            var fed = false
            converter.convert(to: out, error: nil) { _, status in
                if fed {
                    status.pointee = .noDataNow
                    return nil
                }
                fed = true
                status.pointee = .haveData
                return pcm
            }
            guard let channel = out.int16ChannelData else { return }
            sink.append(UnsafeBufferPointer(start: channel[0], count: Int(out.frameLength)))
        }
        engine.prepare()
        try engine.start()
        self.engine = engine
        startedAt = Date()
        phase = .listening
        meterTimer = Timer.scheduledTimer(withTimeInterval: 0.08, repeats: true) { [weak self] _ in
            Task { @MainActor [weak self] in
                guard let self, self.phase == .listening else { return }
                self.level = self.buffer.level
            }
        }
    }

    /// One request at a time, about once a second, for the audio since the
    /// last kept stretch. Each answer replaces the previous guess.
    private func transcribeWhileListening(client: ZorpClient) async {
        var sentThrough = 0
        while phase == .listening, !Task.isCancelled {
            try? await Task.sleep(for: .milliseconds(LiveWindow.intervalMs))
            guard phase == .listening else { break }
            let end = buffer.count
            let window = buffer.samples(from: transcript.windowStart, to: end)
            guard window.count >= LiveWindow.minSamples, end > sentThrough else { continue }
            sentThrough = end
            do {
                let result = try await client.transcribe(audio: Wav.encode(window), mimeType: "audio/wav")
                if LiveWindow.shouldKeep(window) {
                    transcript.commit(result.text, through: end)
                } else {
                    transcript.guess(result.text)
                }
                liveText = transcript.text
            } catch {
                // A missed update is not a lost recording: the audio is still
                // buffered and the next pass or the final one sends it again.
                continue
            }
        }
    }

    static func stageLine(_ stage: String, detail: String) -> String {
        switch stage {
        case "creating_environment": return "Creating the voice environment…"
        case "installing": return "Installing the speech runtime…"
        case "downloading_model": return "Downloading the speech model…"
        case "loading": return "Loading the speech model…"
        case "ready": return "Ready."
        default: return detail.isEmpty ? "Getting voice input ready…" : detail
        }
    }

    static func microphoneAllowed() async -> Bool {
        switch AVCaptureDevice.authorizationStatus(for: .audio) {
        case .authorized: return true
        case .notDetermined: return await AVCaptureDevice.requestAccess(for: .audio)
        default: return false
        }
    }
}

/// When a stretch of audio is sent and when its text is kept.
enum LiveWindow {
    static let sampleRate = 16_000
    static let intervalMs = 900
    /// Half a second: anything shorter is not worth a request.
    static let minSamples = sampleRate / 2
    /// A stretch is kept at the first pause after this long.
    static let keepAfterSamples = sampleRate * 4
    /// And kept regardless at this length, which bounds every request.
    static let maxSamples = sampleRate * 15
    /// The tail that has to be quiet for a moment to count as a pause.
    static let quietTailSamples = sampleRate * 6 / 10
    // ponytail: fixed RMS threshold, a noisy room never reads as a pause and
    // falls back to the 15 second cut. Calibrate from the first second if
    // that turns out to matter.
    static let quietRMS: Double = 450

    static func shouldKeep(_ window: [Int16]) -> Bool {
        if window.count >= maxSamples { return true }
        guard window.count >= keepAfterSamples else { return false }
        return isQuiet(window.suffix(quietTailSamples))
    }

    static func isQuiet(_ samples: ArraySlice<Int16>) -> Bool {
        guard !samples.isEmpty else { return true }
        let sum = samples.reduce(0.0) { $0 + Double($1) * Double($1) }
        return (sum / Double(samples.count)).squareRoot() < quietRMS
    }
}

/// The kept stretches plus the current guess for the one still open.
struct LiveTranscript {
    private(set) var kept: [String] = []
    private(set) var current = ""
    /// Where the open stretch starts, in samples.
    private(set) var windowStart = 0

    var text: String {
        (kept + [current]).map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }
            .filter { !$0.isEmpty }
            .joined(separator: " ")
    }

    mutating func guess(_ text: String) { current = text }

    mutating func commit(_ text: String, through end: Int) {
        kept.append(text)
        current = ""
        windowStart = end
    }

    /// Keep whatever the last guess was, when there is no fresher answer.
    mutating func commitGuess(through end: Int) { commit(current, through: end) }
}

/// The microphone samples, 16 kHz mono, written from the audio thread and
/// read from the main actor.
final class SampleBuffer: @unchecked Sendable {
    private let lock = NSLock()
    private var data: [Int16] = []
    private var lastLevel: Float = 0

    var count: Int { lock.withLock { data.count } }
    var level: Float { lock.withLock { lastLevel } }

    func append(_ samples: UnsafeBufferPointer<Int16>) {
        guard !samples.isEmpty else { return }
        let sum = samples.reduce(0.0) { $0 + Double($1) * Double($1) }
        let rms = (sum / Double(samples.count)).squareRoot() / 32768
        let db = 20 * log10(max(rms, 1e-6))
        lock.withLock {
            data.append(contentsOf: samples)
            lastLevel = Float(max(0, min(1, (db + 50) / 50)))
        }
    }

    func samples(from start: Int, to end: Int? = nil) -> [Int16] {
        lock.withLock {
            let stop = min(end ?? data.count, data.count)
            guard start < stop else { return [] }
            return Array(data[start..<stop])
        }
    }

    func clear() { lock.withLock { data.removeAll(keepingCapacity: true); lastLevel = 0 } }
}

/// A 16-bit mono PCM WAV file, the format the transcribe route takes.
enum Wav {
    static func encode(_ samples: [Int16], sampleRate: Int = LiveWindow.sampleRate) -> Data {
        var out = Data()
        func u32(_ v: UInt32) { withUnsafeBytes(of: v.littleEndian) { out.append(contentsOf: $0) } }
        func u16(_ v: UInt16) { withUnsafeBytes(of: v.littleEndian) { out.append(contentsOf: $0) } }
        let bytes = UInt32(samples.count * 2)
        out.append(contentsOf: Array("RIFF".utf8)); u32(36 + bytes)
        out.append(contentsOf: Array("WAVE".utf8))
        out.append(contentsOf: Array("fmt ".utf8)); u32(16); u16(1); u16(1)
        u32(UInt32(sampleRate)); u32(UInt32(sampleRate * 2)); u16(2); u16(16)
        out.append(contentsOf: Array("data".utf8)); u32(bytes)
        samples.withUnsafeBufferPointer { ptr in
            for s in ptr { u16(UInt16(bitPattern: s)) }
        }
        return out
    }
}
