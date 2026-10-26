import AVFoundation
import Foundation
import Observation

/// Voice input: record from the microphone, get the local speech model
/// ready, and hand back a transcript for the composer.
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
    /// 0...1, from the recorder's own meter. Amplitude only.
    public private(set) var level: Float = 0
    public private(set) var startedAt: Date?
    public private(set) var error: String?

    private var recorder: AVAudioRecorder?
    private var meterTimer: Timer?
    private var fileURL: URL?

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
            if !status.available {
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
        } catch {
            phase = .idle
            self.error = error.localizedDescription
        }
    }

    /// Stop and transcribe. The text comes back; nothing is sent.
    public func stop(client: ZorpClient) async -> String? {
        guard phase == .listening, let recorder, let fileURL else {
            phase = .idle
            return nil
        }
        recorder.stop()
        meterTimer?.invalidate()
        meterTimer = nil
        self.recorder = nil
        level = 0
        startedAt = nil
        phase = .transcribing
        defer {
            try? FileManager.default.removeItem(at: fileURL)
            phase = .idle
        }
        do {
            let audio = try Data(contentsOf: fileURL)
            let result = try await client.transcribe(audio: audio, mimeType: "audio/wav")
            return result.text
        } catch {
            self.error = "Could not transcribe: \(error.localizedDescription)"
            return nil
        }
    }

    public func cancel() {
        recorder?.stop()
        meterTimer?.invalidate()
        meterTimer = nil
        recorder = nil
        if let fileURL { try? FileManager.default.removeItem(at: fileURL) }
        phase = .idle
        level = 0
    }

    private func begin() throws {
        let url = FileManager.default.temporaryDirectory.appendingPathComponent("zorp-voice-\(UUID().uuidString).wav")
        let settings: [String: Any] = [
            AVFormatIDKey: kAudioFormatLinearPCM,
            AVSampleRateKey: 16_000,
            AVNumberOfChannelsKey: 1,
            AVLinearPCMBitDepthKey: 16,
            AVLinearPCMIsFloatKey: false,
            AVLinearPCMIsBigEndianKey: false,
        ]
        let recorder = try AVAudioRecorder(url: url, settings: settings)
        recorder.isMeteringEnabled = true
        guard recorder.record() else {
            throw APIError.transport("the microphone did not start")
        }
        self.recorder = recorder
        fileURL = url
        startedAt = Date()
        phase = .listening
        meterTimer = Timer.scheduledTimer(withTimeInterval: 0.08, repeats: true) { [weak self] _ in
            Task { @MainActor [weak self] in
                guard let self, let recorder = self.recorder else { return }
                recorder.updateMeters()
                let db = recorder.averagePower(forChannel: 0)
                self.level = max(0, min(1, (db + 50) / 50))
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
