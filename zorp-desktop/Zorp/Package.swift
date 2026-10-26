// swift-tools-version: 5.10
import PackageDescription
import Foundation

let packageRoot = Context.packageDirectory

// Swift Testing ships with Xcode, and with the Command Line Tools only as a
// framework outside the default search path. On a machine whose active
// developer directory is the Command Line Tools, the test target has to be
// pointed at that framework by hand. On a machine running Xcode it must not
// be: Xcode's compiler expands `@Test` with its own macro, and mixing that
// with the Command Line Tools' copy of the Testing module fails to compile
// (`extra argument 'sourceLocation'`, `missing argument for parameter
// 'sourceBounds'`), which is what broke CI when the macOS runner image moved
// to a newer Xcode. The active developer directory is read from
// DEVELOPER_DIR when it is set and from xcode-select's link otherwise.
let commandLineTools = "/Library/Developer/CommandLineTools"
let activeDeveloperDir = ProcessInfo.processInfo.environment["DEVELOPER_DIR"]
    ?? (try? FileManager.default.destinationOfSymbolicLink(atPath: "/var/db/xcode_select_link"))
    ?? ""
let usesCommandLineTools = activeDeveloperDir.hasPrefix(commandLineTools)
let testingFrameworks = "\(commandLineTools)/Library/Developer/Frameworks"

let testSwiftSettings: [SwiftSetting] = usesCommandLineTools
    ? [.unsafeFlags(["-F", testingFrameworks])]
    : []
let testLinkerFlags: [String] = usesCommandLineTools
    ? [
        "-F", testingFrameworks,
        "-framework", "Testing",
        "-Xlinker", "-rpath", "-Xlinker", testingFrameworks,
        "-Xlinker", "-rpath", "-Xlinker", "\(commandLineTools)/Library/Developer/usr/lib",
    ]
    : []

let package = Package(
    name: "Zorp",
    platforms: [.macOS(.v14)],
    products: [
        .library(name: "ZorpKit", targets: ["ZorpKit"]),
        .executable(name: "Zorp", targets: ["Zorp"]),
    ],
    targets: [
        .target(
            name: "CZorpBridge",
            path: "CZorpBridge",
            publicHeadersPath: "include"
        ),
        .target(
            name: "ZorpKit",
            dependencies: ["CZorpBridge"],
            path: "Sources/ZorpKit",
            linkerSettings: [
                .unsafeFlags([
                    "-L\(packageRoot)/Frameworks",
                    "-lzorp_desktop_bridge",
                    "-framework", "Security",
                    "-framework", "SystemConfiguration",
                    "-framework", "WebKit",
                    "-framework", "AVFoundation"
                ])
            ]
        ),
        .executableTarget(
            name: "Zorp",
            dependencies: ["ZorpKit", "CZorpBridge"],
            path: "Sources/Zorp",
            linkerSettings: [
                .unsafeFlags([
                    "-L\(packageRoot)/Frameworks",
                    "-lzorp_desktop_bridge",
                    "-framework", "Security",
                    "-framework", "SystemConfiguration",
                    "-framework", "WebKit",
                    "-framework", "AVFoundation"
                ])
            ]
        ),
        .testTarget(
            name: "ZorpTests",
            dependencies: ["ZorpKit", "CZorpBridge"],
            path: "Tests/ZorpTests",
            swiftSettings: testSwiftSettings,
            linkerSettings: [
                .unsafeFlags(["-L\(packageRoot)/Frameworks", "-lzorp_desktop_bridge"] + testLinkerFlags)
            ]
        )
    ]
)
