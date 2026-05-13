import XCTest
@testable import ParrotCore

final class SpeechActivityTrimmerTests: XCTestCase {
    func testTrimsLeadingAndTrailingSilence() {
        let silence = [Float](repeating: 0, count: 16_000)
        let speech = [Float](repeating: 0.05, count: 16_000 / 2)

        let trimmed = SpeechActivityTrimmer.trimForDictation(
            silence + speech + silence,
            paddingMilliseconds: 100
        )

        XCTAssertFalse(trimmed.isEmpty)
        XCTAssertLessThan(trimmed.count, silence.count + speech.count + silence.count)
        XCTAssertGreaterThan(trimmed.count, speech.count)
    }

    func testReturnsEmptyForSilenceOnly() {
        let silence = [Float](repeating: 0, count: 16_000)

        let trimmed = SpeechActivityTrimmer.trimForDictation(silence)

        XCTAssertTrue(trimmed.isEmpty)
    }

    func testLeavesShortBuffersAlone() {
        let samples = [Float](repeating: 0.02, count: 100)

        let trimmed = SpeechActivityTrimmer.trimForDictation(samples)

        XCTAssertEqual(trimmed.count, samples.count)
    }
}
