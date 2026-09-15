import SwiftUI

public struct ConversationView: View {
    @Bindable public var viewModel: SessionViewModel

    public init(viewModel: SessionViewModel) {
        self.viewModel = viewModel
    }

    public var body: some View {
        ScrollViewReader { proxy in
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 16) {
                    ForEach(viewModel.messages) { message in
                        MessageBubbleView(message: message)
                            .id(message.id)
                    }

                    ForEach(viewModel.tools) { tool in
                        ToolActivityView(tool: tool)
                            .id(tool.id)
                    }

                    if viewModel.isWorking {
                        HStack(spacing: 8) {
                            ProgressView()
                                .controlSize(.small)
                            Text("Zorp is thinking...")
                                .font(.caption)
                                .foregroundColor(.secondary)
                        }
                        .padding(.horizontal)
                        .id("working_indicator")
                    }
                }
                .padding(.vertical)
            }
            .onChange(of: viewModel.messages.last?.text) {
                if let lastId = viewModel.messages.last?.id {
                    withAnimation {
                        proxy.scrollTo(lastId, anchor: .bottom)
                    }
                }
            }
        }
    }
}
