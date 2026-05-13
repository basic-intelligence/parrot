import XCTest
@testable import ParrotCore

final class LanguageModelRoutingTests: XCTestCase {
    override func setUp() {
        super.setUp()
        LanguageCatalog.configure([
            LanguageCatalogEntry(code: "en", speechCode: "en", name: "English", nativeName: "English"),
            LanguageCatalogEntry(code: "en-GB", speechCode: "en", name: "English (UK)", nativeName: "English (UK)", variantOf: "en"),
            LanguageCatalogEntry(code: "es", speechCode: "es", name: "Spanish", nativeName: "Español"),
            LanguageCatalogEntry(code: "fr", speechCode: "fr", name: "French", nativeName: "Français"),
            LanguageCatalogEntry(code: "ja", speechCode: "ja", name: "Japanese", nativeName: "日本語"),
            LanguageCatalogEntry(code: "pt", speechCode: "pt", name: "Portuguese", nativeName: "Português"),
            LanguageCatalogEntry(code: "pt-BR", speechCode: "pt", name: "Portuguese (Brazil)", nativeName: "Português (Brasil)", variantOf: "pt"),
        ])
    }

    func testEnglishRoutesToEnglishModelsAndDecodeOptions() {
        let settings = makeSettings(mode: .english)

        XCTAssertEqual(DictationRouting.speechModelKind(for: settings), .english)
        XCTAssertEqual(DictationRouting.cleanupModelKind(for: settings), .standard)
        XCTAssertEqual(DictationRouting.decodeLanguageCode(for: settings), "en")
        XCTAssertFalse(DictationRouting.shouldDetectLanguage(for: settings))
    }

    func testSpecificLanguageRoutesToMultilingualModelsAndForcedLanguage() {
        let settings = makeSettings(mode: .specific, code: "es")

        XCTAssertEqual(DictationRouting.speechModelKind(for: settings), .multilingual)
        XCTAssertEqual(DictationRouting.cleanupModelKind(for: settings), .standard)
        XCTAssertEqual(DictationRouting.decodeLanguageCode(for: settings), "es")
        XCTAssertFalse(DictationRouting.shouldDetectLanguage(for: settings))
    }

    func testEnglishLocaleRoutesToEnglishModelsAndBaseDecodeLanguage() {
        let settings = makeSettings(mode: .specific, code: "en-GB")

        XCTAssertEqual(DictationRouting.speechModelKind(for: settings), .english)
        XCTAssertEqual(DictationRouting.cleanupModelKind(for: settings), .standard)
        XCTAssertEqual(DictationRouting.decodeLanguageCode(for: settings), "en")

        let metadata = DictationRouting.selectedLanguageMetadata(for: settings)
        XCTAssertEqual(metadata.xmlElement, "<dictation_language mode=\"selected\" code=\"en\" locale=\"en-GB\" />")
    }

    func testSpecificLocaleRoutesToMultilingualModelsAndBaseDecodeLanguage() {
        let settings = makeSettings(mode: .specific, code: "pt-BR")

        XCTAssertEqual(DictationRouting.speechModelKind(for: settings), .multilingual)
        XCTAssertEqual(DictationRouting.cleanupModelKind(for: settings), .standard)
        XCTAssertEqual(DictationRouting.decodeLanguageCode(for: settings), "pt")

        let metadata = DictationRouting.selectedLanguageMetadata(for: settings)
        XCTAssertEqual(metadata.xmlElement, "<dictation_language mode=\"selected\" code=\"pt\" locale=\"pt-BR\" />")
    }

    func testSpecificLocaleCanonicalizesInputCasing() {
        let settings = makeSettings(mode: .specific, code: " pt-br ")

        XCTAssertEqual(DictationRouting.decodeLanguageCode(for: settings), "pt")

        let metadata = DictationRouting.selectedLanguageMetadata(for: settings)
        XCTAssertEqual(metadata.xmlElement, "<dictation_language mode=\"selected\" code=\"pt\" locale=\"pt-BR\" />")
    }

    func testDetectLanguageRoutesToMultilingualModelsAndDetection() {
        let settings = makeSettings(mode: .detect)

        XCTAssertEqual(DictationRouting.speechModelKind(for: settings), .multilingual)
        XCTAssertEqual(DictationRouting.cleanupModelKind(for: settings), .standard)
        XCTAssertNil(DictationRouting.decodeLanguageCode(for: settings))
        XCTAssertTrue(DictationRouting.shouldDetectLanguage(for: settings))
    }

    func testSelectedCleanupModelAppliesAcrossLanguageRoutes() {
        var settings = makeSettings(mode: .english)
        settings.cleanupModelId = "cleanup-gemma-4-e2b"

        XCTAssertEqual(DictationRouting.cleanupModelKind(for: settings), .gemma4E2B)

        settings.dictationLanguageMode = .specific
        settings.dictationLanguageCode = "es"

        XCTAssertEqual(DictationRouting.cleanupModelKind(for: settings), .gemma4E2B)
    }

    func testCleanupSlotResolvesThroughCatalogDefault() {
        let descriptor = ModelCatalog.cleanupModel(for: .standard)

        XCTAssertEqual(descriptor.publicID, "cleanup")
        XCTAssertEqual(descriptor.concreteID, ModelCatalog.defaultCleanupModel)
    }

    func testCleanupPublicIDIsStableAlias() {
        let descriptor = ModelCatalog.cleanupModel(for: .standard)

        XCTAssertEqual(descriptor.publicID, "cleanup")
    }

    func testGemmaSelectionDoesNotRewriteDefaultCleanupSlot() {
        let descriptors = ModelCatalog.cleanupModels()

        XCTAssertEqual(descriptors.map(\.publicID), ["cleanup", "cleanup-gemma-4-e2b"])
        XCTAssertEqual(descriptors.first?.concreteID, ModelCatalog.defaultCleanupModel)
        XCTAssertEqual(descriptors.last?.concreteID, ConcreteModelID.gemma4E2B)
    }

    func testCleanupMetadataForSelectedAndDetectedLanguages() {
        let selected = DictationRouting.selectedLanguageMetadata(
            for: makeSettings(mode: .specific, code: "ja")
        )
        XCTAssertEqual(selected.xmlElement, "<dictation_language mode=\"selected\" code=\"ja\" />")

        let detected = DictationRouting.detectedLanguageMetadata(code: "fr")
        XCTAssertEqual(detected.xmlElement, "<dictation_language mode=\"detected\" code=\"fr\" />")
    }

    func testAppleSiliconSpeechCatalogUsesWhisperKitBehindStablePublicIDs() {
        let english = ModelCatalog.speechModel(for: .english, architecture: .appleSilicon)
        let multilingual = ModelCatalog.speechModel(for: .multilingual, architecture: .appleSilicon)

        XCTAssertEqual(english.publicID, "speech")
        XCTAssertEqual(english.concreteID, ConcreteModelID.whisperSmallEnglish)
        XCTAssertNil(english.repoID)
        XCTAssertNil(english.fileName)

        XCTAssertEqual(multilingual.publicID, "speech-multilingual")
        XCTAssertEqual(multilingual.concreteID, ConcreteModelID.whisperLargeV3Multilingual)
        XCTAssertNil(multilingual.repoID)
        XCTAssertNil(multilingual.fileName)
    }

    func testIntelSpeechCatalogUsesWhisperCppBehindStablePublicIDs() {
        let english = ModelCatalog.speechModel(for: .english, architecture: .intel)
        let multilingual = ModelCatalog.speechModel(for: .multilingual, architecture: .intel)

        XCTAssertEqual(english.publicID, "speech")
        XCTAssertEqual(english.concreteID, ConcreteModelID.whisperCppSmallEnglishQ5_1)
        XCTAssertEqual(english.repoID, "ggerganov/whisper.cpp")
        XCTAssertEqual(english.fileName, "ggml-small.en-q5_1.bin")
        XCTAssertEqual(english.expectedBytes, 163_000_000)

        XCTAssertEqual(multilingual.publicID, "speech-multilingual")
        XCTAssertEqual(multilingual.concreteID, ConcreteModelID.whisperCppSmallQ5_1)
        XCTAssertEqual(multilingual.repoID, "ggerganov/whisper.cpp")
        XCTAssertEqual(multilingual.fileName, "ggml-small-q5_1.bin")
        XCTAssertEqual(multilingual.expectedBytes, 163_000_000)
    }

    func testAllLanguageModesUseIntelSafeSpeechRuntimeOnIntel() {
        let english = intelSpeechDescriptor(for: makeSettings(mode: .english))
        let detected = intelSpeechDescriptor(for: makeSettings(mode: .detect))
        let specific = intelSpeechDescriptor(for: makeSettings(mode: .specific, code: "es"))

        XCTAssertEqual(english.publicID, "speech")
        XCTAssertEqual(english.fileName, "ggml-small.en-q5_1.bin")

        XCTAssertEqual(detected.publicID, "speech-multilingual")
        XCTAssertEqual(detected.fileName, "ggml-small-q5_1.bin")

        XCTAssertEqual(specific.publicID, "speech-multilingual")
        XCTAssertEqual(specific.fileName, "ggml-small-q5_1.bin")
    }

    private func intelSpeechDescriptor(for settings: AppSettings) -> SpeechModelDescriptor {
        ModelCatalog.speechModel(
            for: DictationRouting.speechModelKind(for: settings),
            architecture: .intel
        )
    }

    private func makeSettings(
        mode: DictationLanguageMode,
        code: String? = nil
    ) -> AppSettings {
        AppSettings(
            selectedInputUid: nil,
            pushToTalkShortcut: ShortcutSettings(
                displayName: "Fn",
                macosKeyCodes: [63],
                mode: "hold"
            ),
            handsFreeShortcut: ShortcutSettings(
                displayName: "Control + Space",
                macosKeyCodes: [59, 49],
                mode: "toggle"
            ),
            dictationLanguageMode: mode,
            dictationLanguageCode: code,
            cleanupModelId: "cleanup",
            cleanupEnabled: true,
            cleanupPrompt: "",
            dictionaryEntries: [],
            playSounds: true,
            historyEnabled: false,
            launchAtLogin: false,
            onboardingCompleted: false,
            inputMonitoringPermissionShownInOnboarding: false
        )
    }
}
