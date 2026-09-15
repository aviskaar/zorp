import Foundation
import CZorpBridge

public final class BridgeService: @unchecked Sendable {
    public static let shared = BridgeService()

    private var boundPort: UInt16?
    private let lock = NSLock()

    private init() {}

    public func repairPath() {
        _ = zorp_bridge_repair_path()
    }

    public func start(preferredPort: UInt16 = 7777, resourceDir: String? = nil) throws -> UInt16 {
        lock.lock()
        defer { lock.unlock() }

        if let port = boundPort {
            return port
        }

        repairPath()

        var outPort: UInt16 = 0
        let resPathCString = resourceDir?.cString(using: .utf8)
        let status: Int32
        if let resPathCString = resPathCString {
            status = resPathCString.withUnsafeBufferPointer { ptr in
                zorp_bridge_start_server(preferredPort, ptr.baseAddress, &outPort)
            }
        } else {
            status = zorp_bridge_start_server(preferredPort, nil, &outPort)
        }

        guard status == 0, outPort > 0 else {
            throw NSError(domain: "ZorpBridgeError", code: 1, userInfo: [
                NSLocalizedDescriptionKey: "Failed to start background zorp server"
            ])
        }

        boundPort = outPort
        return outPort
    }

    public func stop() {
        lock.lock()
        defer { lock.unlock() }

        guard boundPort != nil else { return }
        zorp_bridge_stop_server()
        boundPort = nil
    }
}
