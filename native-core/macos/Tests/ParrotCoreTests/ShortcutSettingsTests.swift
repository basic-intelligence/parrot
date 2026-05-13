import XCTest
@testable import ParrotCore

final class ShortcutSettingsTests: XCTestCase {
    func testMissingShortcutFlagsDecodeToBackCompatibleDefaults() throws {
        let data = """
        {
          "displayName": "Fn",
          "macosKeyCodes": [63],
          "mode": "hold"
        }
        """.data(using: .utf8)!

        let shortcut = try JSONDecoder.parrot.decode(ShortcutSettings.self, from: data)

        XCTAssertTrue(shortcut.enabled)
        XCTAssertFalse(shortcut.doubleTapToggle)
    }

    func testShortcutFlagsEncode() throws {
        let shortcut = ShortcutSettings(
            displayName: "Fn",
            macosKeyCodes: [63],
            mode: "hold",
            enabled: false,
            doubleTapToggle: true
        )

        let value = try shortcut.jsonValue().objectValue

        XCTAssertEqual(value?["enabled"]?.boolValue, false)
        XCTAssertEqual(value?["doubleTapToggle"]?.boolValue, true)
    }
}
