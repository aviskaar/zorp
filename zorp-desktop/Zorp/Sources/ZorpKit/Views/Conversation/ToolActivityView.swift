import SwiftUI

public struct ToolActivityView: View {
    public let tool: ToolCallItem
    @State private var isExpanded: Bool = false

    public init(tool: ToolCallItem) {
        self.tool = tool
    }

    public var body: some View {
        DisclosureGroup(isExpanded: $isExpanded) {
            if let summary = tool.summary {
                Text(summary)
                    .font(.system(.caption, design: .monospaced))
                    .padding(8)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .background(Color(.controlBackgroundColor))
                    .cornerRadius(4)
            }
        } label: {
            HStack(spacing: 8) {
                if tool.isRunning {
                    ProgressView()
                        .controlSize(.small)
                } else {
                    Image(systemName: "checkmark.circle.fill")
                        .foregroundColor(.green)
                }
                Text(tool.name)
                    .font(.system(.subheadline, design: .monospaced))
                    .bold()
                if let phrase = tool.phrase {
                    Text("— \(phrase)")
                        .font(.subheadline)
                        .foregroundColor(.secondary)
                        .lineLimit(1)
                }
            }
        }
        .padding(8)
        .background(Color(.windowBackgroundColor).opacity(0.6))
        .cornerRadius(6)
        .padding(.horizontal)
    }
}
