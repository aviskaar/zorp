import SwiftUI
import AppKit

public struct MessageBubbleView: View {
    public let message: ChatMessage
    @State private var copied: Bool = false

    public init(message: ChatMessage) {
        self.message = message
    }

    public var body: some View {
        VStack(alignment: message.role == "user" ? .trailing : .leading, spacing: 6) {
            HStack {
                Text(message.role == "user" ? "You" : "Zorp")
                    .font(.caption)
                    .foregroundColor(.secondary)
                    .bold()

                Spacer()

                if message.role != "user" && !message.isStreaming {
                    Button(action: {
                        NSPasteboard.general.clearContents()
                        NSPasteboard.general.setString(message.text, forType: .string)
                        copied = true
                        DispatchQueue.main.asyncAfter(deadline: .now() + 2) {
                            copied = false
                        }
                    }) {
                        Label(copied ? "Copied" : "Copy", systemImage: copied ? "checkmark" : "doc.on.doc")
                            .font(.caption2)
                    }
                    .buttonStyle(.plain)
                    .foregroundColor(.secondary)
                }
            }

            MarkdownView(message.text)
                .padding(12)
                .background(
                    message.role == "user"
                        ? Color.accentColor.opacity(0.12)
                        : Color(.windowBackgroundColor)
                )
                .cornerRadius(8)
                .overlay(
                    RoundedRectangle(cornerRadius: 8)
                        .stroke(Color.secondary.opacity(0.15), lineWidth: 1)
                )
        }
        .padding(.horizontal)
    }
}
