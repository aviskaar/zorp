import SwiftUI

public struct CheckpointCardView: View {
    public let id: String
    public let kind: String
    public let prompt: String
    public let onDecision: (Bool) -> Void

    public init(id: String, kind: String, prompt: String, onDecision: @escaping (Bool) -> Void) {
        self.id = id
        self.kind = kind
        self.prompt = prompt
        self.onDecision = onDecision
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack {
                Image(systemName: "shield.checkered")
                    .foregroundColor(.red)
                Text("Research Checkpoint: \(kind)")
                    .font(.headline)
            }

            Text(prompt)
                .font(.body)

            HStack {
                Spacer()
                Button("Kill Track") {
                    onDecision(false)
                }
                .foregroundColor(.red)

                Button("Continue Research") {
                    onDecision(true)
                }
                .buttonStyle(.borderedProminent)
            }
        }
        .padding()
        .background(Color.red.opacity(0.08))
        .overlay(
            RoundedRectangle(cornerRadius: 8)
                .stroke(Color.red.opacity(0.5), lineWidth: 1)
        )
        .cornerRadius(8)
        .padding(.horizontal)
    }
}
