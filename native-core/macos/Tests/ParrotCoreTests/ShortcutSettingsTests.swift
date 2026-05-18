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
        XCTAssertNil(value?["macosKeyCodes"])
        if case .array(let keyCodes) = value?["platformCodes"]?["macosKeyCodes"] {
            XCTAssertEqual(keyCodes.compactMap(\.numberValue), [63])
        } else {
            XCTFail("Expected platform macOS key codes.")
        }
    }

    func testPlatformNeutralShortcutShapeDecodes() throws {
        let data = """
        {
          "displayName": "Control + Space",
          "mode": "toggle",
          "enabled": true,
          "doubleTapToggle": false,
          "chord": {
            "modifiers": ["control"],
            "key": "space"
          },
          "platformCodes": {
            "macosKeyCodes": [59, 49],
            "windowsVirtualKeys": null,
            "linuxKeyCodes": null
          }
        }
        """.data(using: .utf8)!

        let shortcut = try JSONDecoder.parrot.decode(ShortcutSettings.self, from: data)

        XCTAssertEqual(shortcut.macosKeyCodes, [59, 49])
        XCTAssertEqual(shortcut.chord?.modifiers, ["control"])
    }

    func testAppSettingsMissingPasteTargetPreferenceDefaultsToFalse() throws {
        let data = """
        {
          "selectedInputUid": null,
          "pushToTalkShortcut": {
            "displayName": "Fn",
            "macosKeyCodes": [63],
            "mode": "hold"
          },
          "handsFreeShortcut": {
            "displayName": "Control + Space",
            "macosKeyCodes": [59, 49],
            "mode": "toggle"
          },
          "dictationLanguageMode": "english",
          "dictationLanguageCode": null,
          "cleanupModelId": "cleanup",
          "cleanupEnabled": true,
          "cleanupPrompt": "",
          "dictionaryEntries": [],
          "playSounds": true,
          "historyEnabled": false,
          "launchAtLogin": false,
          "onboardingCompleted": false,
          "inputMonitoringPermissionShownInOnboarding": false
        }
        """.data(using: .utf8)!

        let settings = try JSONDecoder.parrot.decode(AppSettings.self, from: data)
        XCTAssertFalse(settings.pasteIntoRecordingStartWindow)
    }
}
