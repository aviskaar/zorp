#if canImport(Testing)
import Testing
@testable import ZorpKit

@Test func testStartAndStopServer() throws {
    let bridge = BridgeService.shared
    let port = try bridge.start(preferredPort: 17778)
    #expect(port > 0)
    bridge.stop()
}
#elseif canImport(XCTest)
import XCTest
@testable import ZorpKit

final class BridgeServiceTests: XCTestCase {
    func testStartAndStopServer() throws {
        let bridge = BridgeService.shared
        let port = try bridge.start(preferredPort: 17778)
        XCTAssertGreaterThan(port, 0)
        bridge.stop()
    }
}
#endif
