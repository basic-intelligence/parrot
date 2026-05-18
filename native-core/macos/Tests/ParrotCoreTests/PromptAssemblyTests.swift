import XCTest
@testable import ParrotCore

final class PromptAssemblyTests: XCTestCase {
    func testSharedPromptAssemblyFixtures() throws {
        let fixtures = try SharedResources.decode([PromptAssemblyFixture].self, relativePath: "test-fixtures/prompt-assembly.json")

        for fixture in fixtures {
            let prompt = CleanupPromptAssembler.assemble(
                promptFormat: fixture.promptFormat,
                cleanupRules: fixture.cleanupRules,
                dictionaryEntries: fixture.dictionaryEntries,
                transcript: fixture.rawTranscript,
                language: fixture.language.metadata,
                defaultOutputTokens: fixture.defaultOutputTokens
            )

            for expected in fixture.expectedContains {
                XCTAssertTrue(
                    prompt.prompt.contains(expected),
                    "\(fixture.name) missing `\(expected)`"
                )
            }
        }
    }
}

private struct PromptAssemblyFixture: Decodable {
    let name: String
    let promptFormat: CleanupPromptFormat
    let cleanupRules: String
    let dictionaryEntries: [DictionaryEntry]
    let rawTranscript: String
    let language: PromptLanguageFixture
    let defaultOutputTokens: Int32
    let expectedContains: [String]
}

private struct PromptLanguageFixture: Decodable {
    let mode: String
    let code: String?
    let locale: String?
    let name: String?

    var metadata: DictationLanguageMetadata {
        DictationLanguageMetadata(
            mode: mode,
            code: code,
            locale: locale,
            name: name
        )
    }
}
