import SwiftUI
import ZorpKit

@main
struct ZorpApp: App {
    @State private var app: AppState

    init() {
        // The server runs inside this process. If it cannot start, the
        // window still opens and says so, rather than the app vanishing.
        let base: URL
        var failure: String?
        do {
            let port = try BridgeService.shared.start()
            base = URL(string: "http://127.0.0.1:\(port)")!
        } catch {
            base = URL(string: "http://127.0.0.1:7777")!
            failure = "The built-in Zorp server did not start: \(error.localizedDescription)"
        }
        let state = AppState(baseURL: base)
        state.banner = failure
        _app = State(initialValue: state)
    }

    var body: some Scene {
        WindowGroup("Zorp") {
            MainWindowView(app: app)
                .frame(minWidth: 820, minHeight: 520)
        }
        .commands { AppCommands(app: app) }

        Settings {
            SettingsView(app: app)
        }
    }
}
