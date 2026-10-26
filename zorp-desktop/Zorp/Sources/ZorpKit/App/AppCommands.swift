import SwiftUI

public struct AppCommands: Commands {
    let app: AppState

    public init(app: AppState) {
        self.app = app
    }

    public var body: some Commands {
        SidebarCommands()
        InspectorCommands()
        CommandGroup(replacing: .newItem) {
            Button("New Chat") { app.newChat() }
                .keyboardShortcut("n", modifiers: .command)
        }
        CommandMenu("Conversation") {
            Button("Stop") { Task { await app.current.stop() } }
                .keyboardShortcut(".", modifiers: .command)
                .disabled(!app.current.isRunning)
            Button("Compact Earlier Messages") { Task { await app.current.compact() } }
                .disabled(app.current.sessionId == nil || app.current.isRunning)
            Divider()
            Button("Choose Workspace…") { app.wantsWorkspacePicker = true }
                .keyboardShortcut("o", modifiers: [.command, .shift])
        }
    }
}
