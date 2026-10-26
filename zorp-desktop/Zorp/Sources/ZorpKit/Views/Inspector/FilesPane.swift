import AppKit
import PDFKit
import SwiftUI

/// The workspace's files and a preview of the one picked. The listing is
/// the server's, so it shows only what is inside the workspace.
public struct FilesPane: View {
    @Bindable var app: AppState
    @State private var listing: [Artifact] = []
    @State private var truncated = false
    @State private var selected: String?
    @State private var preview: Data?
    @State private var previewError: String?

    public init(app: AppState) {
        self.app = app
    }

    struct FileGroup: Identifiable {
        let folder: String
        let files: [Artifact]
        var id: String { folder }
    }

    private var groups: [FileGroup] {
        let byFolder = Dictionary(grouping: listing) { artifact -> String in
            let parts = artifact.path.split(separator: "/")
            return parts.count > 1 ? parts.dropLast().joined(separator: "/") : ""
        }
        return byFolder.keys.sorted().map { key in
            FileGroup(folder: key, files: byFolder[key]!.sorted { $0.path < $1.path })
        }
    }

    public var body: some View {
        VSplitView {
            List(selection: $selected) {
                if listing.isEmpty {
                    Text(app.workspace?.configured == true ? "No files yet" : "No workspace chosen")
                        .foregroundStyle(.secondary)
                }
                ForEach(groups) { group in
                    Section(group.folder.isEmpty ? "Workspace" : group.folder) {
                        ForEach(group.files) { file in
                            HStack {
                                Image(systemName: Self.symbol(for: file.path))
                                Text((file.path as NSString).lastPathComponent).lineLimit(1)
                                Spacer()
                                Text(ByteCountFormatter.string(fromByteCount: file.bytes, countStyle: .file))
                                    .font(.caption2).foregroundStyle(.secondary)
                            }
                            .tag(file.path)
                            .help(file.path)
                        }
                    }
                }
                if truncated {
                    Text("The list was capped by the server.").font(.caption).foregroundStyle(.secondary)
                }
            }
            .frame(minHeight: 140)

            Group {
                if let selected, let preview {
                    FilePreview(path: selected, data: preview)
                } else if let previewError {
                    Text(previewError).foregroundStyle(.secondary).padding()
                } else {
                    Text("Pick a file to preview").foregroundStyle(.secondary)
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
        .toolbar {
            ToolbarItemGroup {
                Button { Task { await load() } } label: { Label("Refresh", systemImage: "arrow.clockwise") }
                    .help("List the workspace again")
                Button {
                    if let selected, let root = app.workspace?.path {
                        NSWorkspace.shared.activateFileViewerSelecting([URL(fileURLWithPath: root).appendingPathComponent(selected)])
                    }
                } label: { Label("Show in Finder", systemImage: "folder") }
                    .disabled(selected == nil)
                    .help("Show the selected file in Finder")
            }
        }
        .task(id: app.filesGeneration) { await load() }
        .onChange(of: selected) { Task { await open() } }
    }

    private func load() async {
        do {
            let result = try await app.client.listArtifacts()
            listing = result.files
            truncated = result.truncated
        } catch {
            listing = []
        }
        if let selected, listing.contains(where: { $0.path == selected }) {
            await open()
        }
    }

    private func open() async {
        guard let selected else { preview = nil; return }
        do {
            preview = try await app.client.readArtifact(path: selected)
            previewError = nil
        } catch {
            preview = nil
            previewError = error.localizedDescription
        }
    }

    static func symbol(for path: String) -> String {
        switch (path as NSString).pathExtension.lowercased() {
        case "html", "htm": return "globe"
        case "svg", "png", "jpg", "jpeg", "gif", "webp": return "photo"
        case "pdf": return "doc.richtext"
        case "md", "markdown", "txt": return "doc.text"
        case "csv", "tsv", "json": return "tablecells"
        default: return "doc"
        }
    }
}

struct FilePreview: View {
    let path: String
    let data: Data

    var body: some View {
        let ext = (path as NSString).pathExtension.lowercased()
        switch ext {
        case "html", "htm":
            SandboxedWebView(content: String(decoding: data, as: UTF8.self), isHTML: true)
        case "svg":
            SandboxedWebView(content: String(decoding: data, as: UTF8.self), isHTML: false)
        case "png", "jpg", "jpeg", "gif", "webp", "tiff", "heic":
            if let image = NSImage(data: data) {
                ScrollView([.horizontal, .vertical]) { Image(nsImage: image) }
            } else {
                Text("Could not read this image.").foregroundStyle(.secondary)
            }
        case "pdf":
            PDFPreview(data: data)
        case "md", "markdown":
            ScrollView {
                MarkdownView(String(decoding: data, as: UTF8.self)).padding()
            }
        default:
            ScrollView([.horizontal, .vertical]) {
                Text(String(decoding: data.prefix(2_000_000), as: UTF8.self))
                    .font(.system(.caption, design: .monospaced))
                    .textSelection(.enabled)
                    .padding()
                    .frame(maxWidth: .infinity, alignment: .leading)
            }
        }
    }
}

struct PDFPreview: NSViewRepresentable {
    let data: Data

    func makeNSView(context: Context) -> PDFView {
        let view = PDFView()
        view.autoScales = true
        return view
    }

    func updateNSView(_ view: PDFView, context: Context) {
        view.document = PDFDocument(data: data)
    }
}
