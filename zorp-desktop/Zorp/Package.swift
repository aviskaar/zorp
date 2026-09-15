// swift-tools-version: 5.10
import PackageDescription
import Foundation

let packageRoot = Context.packageDirectory

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
            swiftSettings: [
                .unsafeFlags([
                    "-F", "/Library/Developer/CommandLineTools/Library/Developer/Frameworks"
                ])
            ],
            linkerSettings: [
                .unsafeFlags([
                    "-L\(packageRoot)/Frameworks",
                    "-lzorp_desktop_bridge",
                    "-F", "/Library/Developer/CommandLineTools/Library/Developer/Frameworks",
                    "-framework", "Testing",
                    "-Xlinker", "-rpath", "-Xlinker", "/Library/Developer/CommandLineTools/Library/Developer/Frameworks",
                    "-Xlinker", "-rpath", "-Xlinker", "/Library/Developer/CommandLineTools/Library/Developer/usr/lib"


                ])
            ]
        )
    ]
)
