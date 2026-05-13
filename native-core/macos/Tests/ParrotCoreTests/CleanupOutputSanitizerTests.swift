import XCTest
@testable import ParrotCore

final class CleanupOutputSanitizerTests: XCTestCase {
    func testRemovesOrphanThinkClosers() {
        XCTAssertEqual(
            CleanupOutputSanitizer.sanitize("</think>\n\n</think>\n\nHola, me llamo John."),
            "Hola, me llamo John."
        )
    }

    func testRemovesFullThinkBlock() {
        XCTAssertEqual(
            CleanupOutputSanitizer.sanitize("<think>\nreasoning\n</think>\n\nBonjour."),
            "Bonjour."
        )
    }

    func testRemovesNoThinkMarker() {
        XCTAssertEqual(
            CleanupOutputSanitizer.sanitize("/no_think\n\nOi, o meu nome é John."),
            "Oi, o meu nome é John."
        )
    }

    func testKeepsNormalMultilingualOutputIntact() {
        XCTAssertEqual(
            CleanupOutputSanitizer.sanitize("今日はいい天気です。"),
            "今日はいい天気です。"
        )
    }

    func testRemovesGemmaTurnTokens() {
        XCTAssertEqual(
            CleanupOutputSanitizer.sanitize("<|turn>model\nHello, John.<turn|>"),
            "Hello, John."
        )
    }

    func testRemovesGemmaThoughtChannel() {
        XCTAssertEqual(
            CleanupOutputSanitizer.sanitize("<|channel>thought\nreasoning\n<channel|>Hello, John."),
            "Hello, John."
        )
    }

    func testRemovesGemmaLeadingGreaterThanArtifact() {
        XCTAssertEqual(
            CleanupOutputSanitizer.sanitize(">Today I had a good day."),
            "Today I had a good day."
        )
    }

    func testRemovesGemmaLeadingBracketArtifact() {
        XCTAssertEqual(
            CleanupOutputSanitizer.sanitize("] Today I had a good day."),
            "Today I had a good day."
        )
    }

    func testKeepsInternalGreaterThanSymbol() {
        XCTAssertEqual(
            CleanupOutputSanitizer.sanitize("The value is greater than 10, so write x > 10."),
            "The value is greater than 10, so write x > 10."
        )
    }

    func testKeepsIntentionalLeadingBulletList() {
        XCTAssertEqual(
            CleanupOutputSanitizer.sanitize("- Milk\n- Eggs"),
            "- Milk\n- Eggs"
        )
    }

    func testRemovesThoughtChannelThenLeadingArtifact() {
        XCTAssertEqual(
            CleanupOutputSanitizer.sanitize("<|channel>thought\nreasoning\n<channel|>>Today I had a good day."),
            "Today I had a good day."
        )
    }
}
