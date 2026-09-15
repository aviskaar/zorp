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

                    if let approval = viewModel.pendingApproval {
                        ApprovalCardView(id: approval.id, tool: approval.tool, arguments: approval.arguments) { approved in
                            Task {
                                try? await viewModel.client.submitApproval(
                                    sessionId: viewModel.sessionId,
                                    requestId: approval.id,
                                    approved: approved
                                )
                                await MainActor.run {
                                    viewModel.pendingApproval = nil
                                }
                            }
                        }
                        .id("pending_approval_\(approval.id)")
                    }

                    if let checkpoint = viewModel.pendingCheckpoint {
                        CheckpointCardView(id: checkpoint.id, kind: checkpoint.kind, prompt: checkpoint.prompt) { approved in
                            Task {
                                try? await viewModel.client.submitCheckpoint(
                                    sessionId: viewModel.sessionId,
                                    checkpointId: checkpoint.id,
                                    approved: approved
                                )
                                await MainActor.run {
                                    viewModel.pendingCheckpoint = nil
                                }
                            }
                        }
                        .id("pending_checkpoint_\(checkpoint.id)")
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
