import SwiftUI

public struct ComposerView: View {
    @Bindable public var viewModel: SessionViewModel
    @State private var text: String = ""
    @State private var voiceVM = VoiceViewModel()
    @State private var askAtCheckpoints: Bool = false

    public init(viewModel: SessionViewModel) {
        self.viewModel = viewModel
    }

    public var body: some View {
        VStack(spacing: 8) {
            HStack(alignment: .bottom, spacing: 8) {
                TextEditor(text: $text)
                    .font(.body)
                    .frame(minHeight: 36, maxHeight: 120)
                    .padding(6)
                    .background(Color(.controlBackgroundColor))
                    .cornerRadius(6)
                    .overlay(
                        RoundedRectangle(cornerRadius: 6)
                            .stroke(Color.secondary.opacity(0.2), lineWidth: 1)
                    )

                if voiceVM.isRecording {
                    VoiceMeterView(level: voiceVM.audioLevel)
                }

                if voiceVM.isTranscribing {
                    ProgressView()
                        .controlSize(.small)
                        .padding(.bottom, 6)
                } else {
                    Button(action: {
                        if voiceVM.isRecording {
                            voiceVM.stopRecording(client: viewModel.client) { transcribed in
                                if !transcribed.isEmpty {
                                    if !text.isEmpty {
                                        text += " " + transcribed
                                    } else {
                                        text = transcribed
                                    }
                                }
                            }
                        } else {
                            voiceVM.startRecording()
                        }
                    }) {
                        Image(systemName: voiceVM.isRecording ? "stop.circle.fill" : "mic.fill")
                            .font(.title3)
                            .foregroundColor(voiceVM.isRecording ? .red : .primary)
                    }
                    .buttonStyle(.plain)
                    .help(voiceVM.isRecording ? "Stop Recording" : "Voice Input (Qwen3-ASR)")
                    .padding(.bottom, 6)
                }

                if viewModel.isWorking {
                    Button(action: {
                        Task {
                            try? await viewModel.client.stopTurn(sessionId: viewModel.sessionId)
                        }
                    }) {
                        Image(systemName: "stop.circle.fill")
                            .font(.title2)
                            .foregroundColor(.red)
                    }
                    .buttonStyle(.plain)
                    .help("Stop Turn (Escape)")
                    .keyboardShortcut(.escape, modifiers: [])
                    .padding(.bottom, 4)
                } else {
                    Button(action: {
                        let prompt = text.trimmingCharacters(in: .whitespacesAndNewlines)
                        guard !prompt.isEmpty else { return }
                        text = ""
                        Task {
                            await viewModel.send(prompt: prompt)
                        }
                    }) {
                        Image(systemName: "arrow.up.circle.fill")
                            .font(.title2)
                            .foregroundColor(.accentColor)
                    }
                    .buttonStyle(.plain)
                    .disabled(text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                    .help("Send Message (Return)")
                    .padding(.bottom, 4)
                }
            }

            HStack {
                Toggle("Ask at each research checkpoint", isOn: $askAtCheckpoints)
                    .font(.caption2)
                    .foregroundColor(.secondary)
                    .onChange(of: askAtCheckpoints) {
                        Task {
                            try? await viewModel.client.setAutoApprove(
                                sessionId: viewModel.sessionId,
                                autoApprove: !askAtCheckpoints
                            )
                        }
                    }

                Spacer()
            }
        }
        .padding(10)
        .background(Color(.windowBackgroundColor).opacity(0.9))
    }
}
