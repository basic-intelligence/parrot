import XCTest
@testable import ParrotCore

final class SpeechActivityTrimmerTests: XCTestCase {
    func testSharedFixtures() throws {
        let fixtures = try SharedResources.decode([SpeechActivityTrimmerFixture].self, relativePath: "test-fixtures/speech-activity-trimmer.json")

        for fixture in fixtures {
            XCTAssertEqual(
                SpeechActivityTrimmer.trimForDictation(
                    fixture.samples,
                    sampleRate: fixture.sampleRateHz,
                    frameMilliseconds: fixture.frameMilliseconds,
                    paddingMilliseconds: fixture.paddingMilliseconds,
                    minimumSpeechMilliseconds: fixture.minimumSpeechMilliseconds
                ),
                fixture.expected,
                fixture.name
            )
        }
    }

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

private struct SpeechActivityTrimmerFixture: Decodable {
    let name: String
    let samples: [Float]
    let sampleRateHz: Int
    let frameMilliseconds: Int
    let paddingMilliseconds: Int
    let minimumSpeechMilliseconds: Int
    let expected: [Float]
}
