import SwiftUI
import ZorpKit

@main
struct ZorpApp: App {
    @State private var appState: AppState

    init() {
        let port: UInt16
        do {
            port = try BridgeService.shared.start()
        } catch {
            fatalError("Failed to initialize Zorp bridge: \(error)")
        }
        let base = URL(string: "http://127.0.0.1:\(port)")!
        _appState = State(initialValue: AppState(baseURL: base))
    }

    var body: some Scene {
        WindowGroup {
            MainWindowView(appState: appState)
                .onAppear {
                    Task {
                        await appState.createNewSession()
                    }
                }
        }
        .commands {
            AppCommands(appState: appState)
        }
    }
}
