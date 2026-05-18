import XCTest
@testable import ParrotCore

final class PermissionSnapshotTests: XCTestCase {
    func testPermissionSnapshotEncodesInputMonitoringAndAllGranted() throws {
        let requirements = [
            PermissionRequirementDTO(
                kind: .microphone,
                title: "Microphone",
                description: "Record your voice locally for dictation.",
                state: .granted,
                required: true,
                requestable: true,
                opensSettings: true
            ),
            PermissionRequirementDTO(
                kind: .accessibility,
                title: "Accessibility",
                description: "Consume the Parrot shortcut event and paste the finished text.",
                state: .granted,
                required: true,
                requestable: true,
                opensSettings: true
            ),
            PermissionRequirementDTO(
                kind: .inputMonitoring,
                title: "Input Monitoring",
                description: "Some Macs require this so Parrot Core can listen for your shortcut while you use other apps.",
                state: .denied,
                required: false,
                requestable: true,
                opensSettings: true
            ),
        ]
        let snapshot = PermissionSnapshotDTO(
            requirements: requirements,
            allRequiredGranted: true,
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
        XCTAssertEqual(object["allRequiredGranted"] as? Bool, true)
        XCTAssertEqual((object["requirements"] as? [[String: Any]])?.count, 3)
    }
}
