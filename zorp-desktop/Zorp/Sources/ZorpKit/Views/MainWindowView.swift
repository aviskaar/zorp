import SwiftUI

public struct MainWindowView: View {
    @Bindable public var appState: AppState
    @State private var inspectorPresented: Bool = false

    public init(appState: AppState) {
        self.appState = appState
    }

    public var body: some View {
        NavigationSplitView {
            SidebarView(appState: appState)
                .navigationSplitViewColumnWidth(min: 220, ideal: 260, max: 360)
        } detail: {
            if let vm = appState.activeSessionVM {
                VStack(spacing: 0) {
                    // Header Bar
                    HStack(spacing: 12) {
                        Text("Session")
                            .font(.headline)
                            .bold()

                        Spacer()

                        // Context Meter
                        if vm.usedTokens > 0 {
                            HStack(spacing: 4) {
                                Image(systemName: "gauge.with.needle")
                                    .font(.caption)
                                if let limit = vm.limitTokens {
                                    Text("\(vm.usedTokens) / \(limit)")
                                } else {
                                    Text("\(vm.usedTokens) tokens (\(vm.contextSource))")
                                }
                            }
                            .font(.caption2)
                            .padding(.horizontal, 8)
                            .padding(.vertical, 4)
                            .background(Color.secondary.opacity(0.12))
                            .cornerRadius(12)
                        }

                        // Bolt / Investigate Button
                        Button(action: {
                            Task {
                                try? await vm.client.startInvestigate(sessionId: vm.sessionId, attempts: 3)
                            }
                        }) {
                            Label("Bolt", systemImage: "bolt.fill")
                                .foregroundColor(.yellow)
                        }
                        .help("Run Investigation Bolt (3 attempts)")
                    }
                    .padding(.horizontal, 16)
                    .padding(.vertical, 8)
                    .background(Color(.windowBackgroundColor).opacity(0.6))

                    Divider()

                    ConversationView(viewModel: vm)
                }
            } else {
                ContentUnavailableView(
                    "No Session Selected",
                    systemImage: "bubble.left.and.bubble.right",
                    description: Text("Select a session from the sidebar or start a new one.")
                )
            }
        }
        .inspector(isPresented: $inspectorPresented) {
            VStack(spacing: 16) {
                Text("Inspector")
                    .font(.headline)
                Spacer()
                Text("Artifacts and ledger will appear here.")
                    .font(.caption)
                    .foregroundColor(.secondary)
                Spacer()
            }
            .frame(minWidth: 260)
        }
        .toolbar {
            ToolbarItem(placement: .primaryAction) {
                Button(action: { inspectorPresented.toggle() }) {
                    Image(systemName: "sidebar.trailing")
                }
                .help("Toggle Inspector (⌘\\)")
            }
        }
        .onChange(of: appState.activeSessionId) {
            if let id = appState.activeSessionId {
                appState.selectSession(id)
            }
        }
    }
}
