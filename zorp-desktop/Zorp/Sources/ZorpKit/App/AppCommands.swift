import SwiftUI

public struct AppCommands: Commands {
    public let appState: AppState

    public init(appState: AppState) {
        self.appState = appState
    }

    public var body: some Commands {
        SidebarCommands()
        CommandGroup(replacing: .newItem) {
            Button("New Session") {
                Task {
                    await appState.createNewSession()
                }
            }
            .keyboardShortcut("n", modifiers: .command)
        }
    }
}
