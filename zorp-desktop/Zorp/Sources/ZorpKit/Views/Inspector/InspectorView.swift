import SwiftUI

public struct InspectorView: View {
    @Bindable public var viewModel: SessionViewModel
    @State private var selectedTab: Int = 0
    @State private var artifacts: [ArtifactItem] = []
    @State private var selectedArtifactPath: String?
    @State private var artifactContent: String?
    @State private var isLoadingArtifact: Bool = false

    public init(viewModel: SessionViewModel) {
        self.viewModel = viewModel
    }

    public var body: some View {
        VStack(spacing: 0) {
            Picker("View", selection: $selectedTab) {
                Text("Artifacts").tag(0)
                Text("Ledger").tag(1)
            }
            .pickerStyle(.segmented)
            .padding(10)

            Divider()

            if selectedTab == 0 {
                artifactsPane
            } else {
                ledgerPane
            }
        }
        .frame(minWidth: 280)
        .task {
            await loadArtifacts()
        }
    }

    @ViewBuilder
    private var artifactsPane: some View {
        if artifacts.isEmpty {
            ContentUnavailableView(
                "No Artifacts",
                systemImage: "doc.text",
                description: Text("Artifacts produced during turns will appear here.")
            )
        } else {
            VStack(spacing: 0) {
                List(artifacts, selection: $selectedArtifactPath) { item in
                    HStack {
                        Image(systemName: item.path.hasSuffix(".svg") ? "photo" : (item.path.hasSuffix(".html") ? "globe" : "doc.text"))
                        Text(item.path)
                            .font(.caption)
                    }
                    .tag(item.path)
                }
                .frame(maxHeight: 160)

                Divider()

                if isLoadingArtifact {
                    ProgressView()
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                } else if let content = artifactContent, let path = selectedArtifactPath {
                    if path.hasSuffix(".html") {
                        SandboxedWebView(content: content, isHTML: true)
                    } else if path.hasSuffix(".svg") {
                        SandboxedWebView(content: content, isHTML: false)
                    } else {
                        ScrollView {
                            Text(content)
                                .font(.system(.caption, design: .monospaced))
                                .padding()
                                .frame(maxWidth: .infinity, alignment: .leading)
                        }
                    }
                } else {
                    Text("Select an artifact to preview")
                        .font(.caption)
                        .foregroundColor(.secondary)
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                }
            }
            .onChange(of: selectedArtifactPath) {
                if let path = selectedArtifactPath {
                    Task {
                        await loadArtifactContent(path: path)
                    }
                }
            }
        }
    }

    @ViewBuilder
    private var ledgerPane: some View {
        if let experiments = viewModel.investigateState?.ledger?.experiments, !experiments.isEmpty {
            LedgerTableView(experiments: experiments)
        } else {
            ContentUnavailableView(
                "No Ledger Data",
                systemImage: "tablecells",
                description: Text("Aryabhatta discovery experiments will record here.")
            )
        }
    }

    private func loadArtifacts() async {
        do {
            let list = try await viewModel.client.listArtifacts()
            await MainActor.run {
                self.artifacts = list
                if selectedArtifactPath == nil, let first = list.first {
                    self.selectedArtifactPath = first.path
                }
            }
        } catch {
            print("Failed to load artifacts: \(error)")
        }
    }

    private func loadArtifactContent(path: String) async {
        isLoadingArtifact = true
        do {
            let content = try await viewModel.client.readArtifact(path: path)
            await MainActor.run {
                self.artifactContent = content
                self.isLoadingArtifact = false
            }
        } catch {
            await MainActor.run {
                self.artifactContent = "Failed to load artifact: \(error.localizedDescription)"
                self.isLoadingArtifact = false
            }
        }
    }
}
