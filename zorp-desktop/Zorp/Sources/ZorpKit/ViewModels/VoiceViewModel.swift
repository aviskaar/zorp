import AVFoundation
import Foundation
import Observation

@Observable
public final class VoiceViewModel: @unchecked Sendable {
    public var isRecording: Bool = false
    public var audioLevel: Float = 0.0
    public var isTranscribing: Bool = false

    private var audioEngine: AVAudioEngine?
    private var inputNode: AVAudioInputNode?
    private var audioFile: AVAudioFile?
    private var tempFileURL: URL?

    public init() {}

    public func startRecording() {
        let engine = AVAudioEngine()
        let input = engine.inputNode
        let format = input.outputFormat(forBus: 0)

        let tempDir = FileManager.default.temporaryDirectory
        let fileURL = tempDir.appendingPathComponent("zorp_voice_\(UUID().uuidString).wav")
        self.tempFileURL = fileURL

        do {
            self.audioFile = try AVAudioFile(forWriting: fileURL, settings: format.settings)
        } catch {
            print("Failed to create temporary audio file: \(error)")
            return
        }

        input.installTap(onBus: 0, bufferSize: 1024, format: format) { [weak self] buffer, _ in
            guard let self = self else { return }

            // Write audio buffer to file for transcription
            try? self.audioFile?.write(from: buffer)

            guard let channelData = buffer.floatChannelData?[0] else { return }
            let frames = Int(buffer.frameLength)
            var sum: Float = 0
            for i in 0..<frames {
                sum += channelData[i] * channelData[i]
            }
            let rms = sqrt(sum / Float(max(frames, 1)))
            DispatchQueue.main.async {
                self.audioLevel = min(max(rms * 10, 0), 1)
            }
        }

        do {
            try engine.start()
            self.audioEngine = engine
            self.inputNode = input
            self.isRecording = true
        } catch {
            print("Failed to start audio engine: \(error)")
        }
    }

    public func stopRecording(client: ZorpClient? = nil, onTranscribed: ((String) -> Void)? = nil) {
        inputNode?.removeTap(onBus: 0)
        audioEngine?.stop()
        audioEngine = nil
        inputNode = nil
        audioFile = nil
        isRecording = false
        audioLevel = 0.0

        guard let fileURL = tempFileURL, let client = client else { return }
        isTranscribing = true

        Task {
            do {
                let data = try Data(contentsOf: fileURL)
                let text = try await client.transcribeVoice(audioData: data, mimeType: "audio/wav")
                await MainActor.run {
                    self.isTranscribing = false
                    onTranscribed?(text)
                }
            } catch {
                print("Voice transcription failed: \(error)")
                await MainActor.run {
                    self.isTranscribing = false
                }
            }
            try? FileManager.default.removeItem(at: fileURL)
        }
    }
}
