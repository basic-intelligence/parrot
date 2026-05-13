import XCTest
@testable import ParrotCore

final class ContextualPasteFormatterTests: XCTestCase {
    func testAddsSpaceAfterSentencePunctuationBeforeWord() {
        XCTAssertEqual(
            ContextualPasteFormatter.format("World.", precedingContext: "Hello."),
            " World."
        )
    }

    func testAddsSpaceAfterClosingQuote() {
        XCTAssertEqual(
            ContextualPasteFormatter.format("yes", precedingContext: "He said “hello.”"),
            " Yes"
        )
    }

    func testLeavesTextAttachedAfterAmbiguousQuote() {
        XCTAssertEqual(
            ContextualPasteFormatter.format("hello", precedingContext: "\""),
            "hello"
        )
    }

    func testLeavesTextUnchangedWhenContextAlreadyEndsInWhitespace() {
        XCTAssertEqual(
            ContextualPasteFormatter.format("world", precedingContext: "Hello, "),
            "world"
        )
    }

    func testAddsSpaceAfterWordBeforeWord() {
        XCTAssertEqual(
            ContextualPasteFormatter.format("world", precedingContext: "Hello"),
            " world"
        )
    }

    func testDoesNotAddSpaceAtStartOfNewLineAfterSentencePunctuation() {
        XCTAssertEqual(
            ContextualPasteFormatter.format(
                "I like coding.",
                precedingContext: "Hello my name is John.\n"
            ),
            "I like coding."
        )
    }

    func testUsesOnlyCurrentLineForSpacingDecision() {
        XCTAssertEqual(
            ContextualPasteFormatter.format(
                "coding.",
                precedingContext: "Hello my name is John.\nI like"
            ),
            " coding."
        )
    }

    func testCapitalizesAfterSentenceTerminator() {
        XCTAssertEqual(
            ContextualPasteFormatter.format("hello again.", precedingContext: "Done. "),
            "Hello again."
        )
    }

    func testCapitalizesWhenAddingSpaceAfterSentenceTerminator() {
        XCTAssertEqual(
            ContextualPasteFormatter.format("hello again.", precedingContext: "Done."),
            " Hello again."
        )
    }

    func testDoesNotCapitalizeCamelCaseBrand() {
        XCTAssertEqual(
            ContextualPasteFormatter.format("iPhone setup is done.", precedingContext: "Done. "),
            "iPhone setup is done."
        )
    }

    func testTrimsLeadingHorizontalWhitespaceBeforeWordLikePaste() {
        XCTAssertEqual(
            ContextualPasteFormatter.format(" hello", precedingContext: "Hi,"),
            " hello"
        )
    }

    func testCapitalizesTrimmedLeadingHorizontalWhitespaceAfterSentenceTerminator() {
        XCTAssertEqual(
            ContextualPasteFormatter.format(" hello", precedingContext: "Hi."),
            " Hello"
        )
    }

    func testPreservesIntentionalLeadingNewline() {
        XCTAssertEqual(
            ContextualPasteFormatter.format("\n- Milk\n- Eggs", precedingContext: "Shopping list:"),
            "\n- Milk\n- Eggs"
        )
    }

    func testLeavesPunctuationOnlyDictationAttachedToPreviousWord() {
        XCTAssertEqual(
            ContextualPasteFormatter.format(",", precedingContext: "Hello"),
            ","
        )
    }

    func testLeavesTextAttachedAfterOpeningParenthesis() {
        XCTAssertEqual(
            ContextualPasteFormatter.format("hello", precedingContext: "("),
            "hello"
        )
    }

    func testLeavesTextUnchangedWithoutContext() {
        XCTAssertEqual(
            ContextualPasteFormatter.format("hello", precedingContext: nil),
            "hello"
        )
        XCTAssertEqual(
            ContextualPasteFormatter.format("hello", precedingContext: ""),
            "hello"
        )
    }
}
