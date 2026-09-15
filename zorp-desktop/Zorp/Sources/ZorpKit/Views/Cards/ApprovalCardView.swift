import SwiftUI

public struct ApprovalCardView: View {
    public let id: String
    public let tool: String
    public let arguments: String
    public let onDecision: (Bool) -> Void

    public init(id: String, tool: String, arguments: String, onDecision: @escaping (Bool) -> Void) {
        self.id = id
        self.tool = tool
        self.arguments = arguments
        self.onDecision = onDecision
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack {
                Image(systemName: "exclamationmark.triangle.fill")
                    .foregroundColor(.yellow)
                Text("Approval Required")
                    .font(.headline)
                Spacer()
                Text(tool)
                    .font(.system(.subheadline, design: .monospaced))
                    .padding(.horizontal, 6)
                    .padding(.vertical, 2)
                    .background(Color.yellow.opacity(0.2))
                    .cornerRadius(4)
            }

            Text("Arguments:")
                .font(.caption)
                .bold()

            Text(arguments)
                .font(.system(.caption, design: .monospaced))
                .padding(8)
                .frame(maxWidth: .infinity, alignment: .leading)
                .background(Color(.controlBackgroundColor))
                .cornerRadius(4)

            HStack {
                Spacer()
                Button("Deny (⌘D)") {
                    onDecision(false)
                }
                .keyboardShortcut("d", modifiers: .command)

                Button("Approve (⌘Y)") {
                    onDecision(true)
                }
                .buttonStyle(.borderedProminent)
                .keyboardShortcut("y", modifiers: .command)
            }
        }
        .padding()
        .background(Color.yellow.opacity(0.08))
        .overlay(
            RoundedRectangle(cornerRadius: 8)
                .stroke(Color.yellow.opacity(0.5), lineWidth: 1)
        )
        .cornerRadius(8)
        .padding(.horizontal)
    }
}
