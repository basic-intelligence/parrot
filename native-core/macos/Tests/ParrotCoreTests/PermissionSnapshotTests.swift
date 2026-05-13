import XCTest
@testable import ParrotCore

final class PermissionSnapshotTests: XCTestCase {
    func testPermissionSnapshotEncodesInputMonitoringAndAllGranted() throws {
        let snapshot = PermissionSnapshotDTO(
            microphone: .granted,
            accessibility: .granted,
            inputMonitoring: .denied,
            allGranted: true
        )

        let data = try JSONEncoder.parrot.encode(snapshot)
        let object = try XCTUnwrap(
            JSONSerialization.jsonObject(with: data) as? [String: Any]
        )

        XCTAssertEqual(object["microphone"] as? String, "granted")
        XCTAssertEqual(object["accessibility"] as? String, "granted")
        XCTAssertEqual(object["inputMonitoring"] as? String, "denied")
        XCTAssertEqual(object["allGranted"] as? Bool, true)
    }
}
