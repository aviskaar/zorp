import SwiftUI

struct ComposerView: View {
    @Bindable var app: AppState
    @Bindable var session: SessionViewModel
    @State private var text = ""
    @State private var useMemory = false
    @State private var voice = VoiceRecorder()
    @State private var showPanel = false
    @State private var showInvestigate = false
    @FocusState private var focused: Bool

    private var trimmed: String { text.trimmingCharacters(in: .whitespacesAndNewlines) }

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            if !session.queue.isEmpty { queueList }
            voiceStatus
            HStack(alignment: .bottom, spacing: 10) {
                editor
                if session.isRunning {
                    Button {
                        Task { await session.stop() }
                    } label: {
                        Image(systemName: "stop.circle.fill").font(.system(size: 26)).foregroundStyle(.red)
                    }
                    .buttonStyle(.plain)
                    .keyboardShortcut(.escape, modifiers: [])
                    .help("Stop (Esc)")
                }
                Button(action: submit) {
                    Image(systemName: "arrow.up.circle.fill").font(.system(size: 26))
                }
                .buttonStyle(.plain)
                .foregroundStyle(trimmed.isEmpty ? Color.secondary : Color.accentColor)
                .disabled(trimmed.isEmpty)
                .help(session.isRunning ? "Queue (Return)" : "Send (Return)")
            }
            controls
        }
        .padding(12)
        .frame(maxWidth: 820)
        .frame(maxWidth: .infinity)
        .onAppear { focused = true }
        .sheet(isPresented: $showPanel) {
            PanelSheet(app: app, session: session, isPresented: $showPanel)
        }
        .sheet(isPresented: $showInvestigate) {
            InvestigateSheet(app: app, session: session, question: trimmed, isPresented: $showInvestigate) {
                text = ""
            }
        }
    }

    private var editor: some View {
        ZStack(alignment: .topLeading) {
            if text.isEmpty {
                Text(session.isRunning ? "Queue a follow-up…" : "Message Zorp")
                    .foregroundStyle(.tertiary)
                    .padding(.horizontal, 5)
                    .padding(.vertical, 8)
                    .allowsHitTesting(false)
            }
            TextEditor(text: $text)
                .font(.body)
                .scrollContentBackground(.hidden)
                .frame(minHeight: 36, maxHeight: 160)
                .fixedSize(horizontal: false, vertical: true)
                .focused($focused)
                .onKeyPress(.return, phases: .down) { press in
                    if press.modifiers.contains(.shift) { return .ignored }
                    submit()
                    return .handled
                }
        }
        .padding(6)
        .background(Color(nsColor: .textBackgroundColor))
        .clipShape(RoundedRectangle(cornerRadius: 10))
        .overlay(RoundedRectangle(cornerRadius: 10).stroke(Color.secondary.opacity(0.25)))
    }

    private var queueList: some View {
        VStack(alignment: .leading, spacing: 4) {
            ForEach(Array(session.queue.enumerated()), id: \.offset) { index, queued in
                HStack {
                    Image(systemName: "clock").foregroundStyle(.secondary)
                    Text(queued).lineLimit(1).foregroundStyle(.secondary)
                    Spacer()
                    Button {
                        session.queue.remove(at: index)
                    } label: { Image(systemName: "xmark.circle") }
                        .buttonStyle(.borderless)
                        .help("Don't send this one")
                }
                .font(.caption)
            }
        }
    }

    @ViewBuilder
    private var voiceStatus: some View {
        switch voice.phase {
        case .preparing(let line):
            HStack(spacing: 8) {
                ProgressView().controlSize(.small)
                Text(line).font(.caption).foregroundStyle(.secondary)
                Spacer()
                Button("Cancel") { voice.cancel() }.controlSize(.small)
            }
        case .listening:
            HStack(spacing: 8) {
                Circle().fill(Color.red).frame(width: 8, height: 8)
                Text("Listening").font(.caption)
                if let started = voice.startedAt {
                    Text(started, style: .timer).font(.caption).monospacedDigit().foregroundStyle(.secondary)
                }
                LevelMeter(level: voice.level)
                Spacer()
            }
        case .transcribing:
            HStack(spacing: 8) {
                ProgressView().controlSize(.small)
                Text("Transcribing…").font(.caption).foregroundStyle(.secondary)
            }
        case .idle:
            if let error = voice.error {
                Text(error).font(.caption).foregroundStyle(.red)
            }
        }
    }

    private var controls: some View {
        HStack(spacing: 14) {
            if app.voice != nil {
                Button {
                    Task { await toggleVoice() }
                } label: {
                    Image(systemName: voice.isListening ? "stop.circle.fill" : "mic")
                        .foregroundStyle(voice.isListening ? Color.red : Color.secondary)
                }
                .buttonStyle(.borderless)
                .disabled(voice.phase == .transcribing || isPreparing)
                .help(voice.isListening ? "Stop listening" : (app.voice?.available == true || app.voice?.setupAvailable == true
                    ? "Voice input, transcribed on this Mac" : (app.voice?.detail ?? "Voice input is unavailable")))
            }
            Button {
                showInvestigate = true
            } label: {
                Image(systemName: "bolt.fill").foregroundStyle(app.investigate?.available == true ? Color.yellow : Color.secondary)
            }
            .buttonStyle(.borderless)
            .disabled(session.isRunning || app.investigate?.available != true)
            .help(app.investigate?.available == true
                ? "Investigate: pre-registered attempts, then a write-up"
                : "Investigate is not in this build")
            Button {
                showPanel = true
            } label: {
                Image(systemName: "person.3")
            }
            .buttonStyle(.borderless)
            .disabled(session.isRunning)
            .help("Ask a review panel to read something")
            if app.recall?.memory == true {
                Toggle(isOn: $useMemory) {
                    Label("Memory", systemImage: "brain").font(.caption)
                }
                .toggleStyle(.button)
                .controlSize(.small)
                .help("Quote earlier conversations into this message. Applies to the next message only.")
            }
            Toggle(isOn: Binding(
                get: { session.autoApprove },
                set: { on in Task { await session.setAutoApprove(on) } })
            ) {
                Text("Allow tool calls without asking").font(.caption)
            }
            .toggleStyle(.checkbox)
            .help("Stands approvals down for this conversation only. A new chat asks again.")
            Spacer()
            if let fraction = session.contextFraction {
                ContextMeter(fraction: fraction, used: session.usedTokens, limit: session.limitTokens ?? 0)
            } else if session.usedTokens > 0 {
                Text("\(session.usedTokens) tokens")
                    .font(.caption).foregroundStyle(.secondary).monospacedDigit()
            }
        }
        .foregroundStyle(.secondary)
    }

    private var isPreparing: Bool {
        if case .preparing = voice.phase { return true }
        return false
    }

    private func toggleVoice() async {
        if voice.isListening {
            if let transcript = await voice.stop(client: app.client), !transcript.isEmpty {
                text = text.isEmpty ? transcript : text + " " + transcript
                focused = true
            }
        } else {
            await voice.start(client: app.client)
        }
    }

    private func submit() {
        let message = trimmed
        guard !message.isEmpty else { return }
        text = ""
        let memory = useMemory
        useMemory = false
        Task { await session.send(message, memory: memory) }
    }
}

struct LevelMeter: View {
    let level: Float

    var body: some View {
        HStack(spacing: 2) {
            ForEach(0..<10, id: \.self) { index in
                RoundedRectangle(cornerRadius: 1)
                    .fill(Float(index) / 10 < level ? Color.accentColor : Color.secondary.opacity(0.25))
                    .frame(width: 3, height: CGFloat(5 + index))
            }
        }
        .frame(height: 16)
    }
}
