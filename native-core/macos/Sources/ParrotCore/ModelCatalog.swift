import Foundation

enum ModelRole: String, Codable, Sendable {
    case speech
    case cleanup
}

enum ModelHostArchitecture: Sendable {
    case appleSilicon
    case intel

    static var current: ModelHostArchitecture {
        #if arch(x86_64)
        .intel
        #else
        .appleSilicon
        #endif
    }
}

enum ModelSlot {
    static let englishSpeech = "speech"
    static let multilingualSpeech = "speech-multilingual"
}

enum ConcreteModelID {
    static let whisperSmallEnglish = "whisperkit-openai-whisper-small-en"
    static let whisperLargeV3Multilingual = "whisperkit-openai-whisper-large-v3"
    static let whisperCppSmallEnglishQ5_1 = "whispercpp-ggml-small-en-q5-1"
    static let whisperCppSmallQ5_1 = "whispercpp-ggml-small-q5-1"
    static let qwen35_2BQ8_0 = "llama-qwen3-5-2b-q8-0"
    static let gemma4E2B = "llama-gemma-4-e2b-q8-0"
}

enum CleanupPromptFormat: Sendable {
    case qwen3ChatML
    case gemma4Turns
}

struct LlamaSamplerConfiguration: Sendable {
    let topK: Int32
    let topP: Float
    let minP: Float
    let temperature: Float
    let repeatPenalty: Float
    let frequencyPenalty: Float
    let presencePenalty: Float

    static let qwen35Cleanup = LlamaSamplerConfiguration(
        topK: 1,
        topP: 1.0,
        minP: 0,
        temperature: 0.05,
        repeatPenalty: 1.05,
        frequencyPenalty: 0,
        presencePenalty: 0
    )

    static let gemma4Cleanup = LlamaSamplerConfiguration(
        topK: 1,
        topP: 1.0,
        minP: 0,
        temperature: 0.05,
        repeatPenalty: 1.05,
        frequencyPenalty: 0,
        presencePenalty: 0
    )
}

struct SpeechModelDescriptor: Sendable {
    let publicID: String
    let concreteID: String
    let modelID: String
    let repoID: String?
    let fileName: String?
    let displayName: String
    let subtitle: String
    let expectedBytes: Int64
}

struct CleanupModelDescriptor: Sendable {
    let publicID: String
    let concreteID: String
    let repoID: String
    let fileName: String
    let displayName: String
    let subtitle: String
    let expectedBytes: Int64
    let promptFormat: CleanupPromptFormat
    let samplerConfiguration: LlamaSamplerConfiguration
    let contextTokens: Int32
    let outputTokens: Int32
}

enum ModelCatalog {
    static let defaultEnglishSpeechModel = ConcreteModelID.whisperSmallEnglish
    static let defaultMultilingualSpeechModel = ConcreteModelID.whisperLargeV3Multilingual
    static let defaultCleanupModel = ConcreteModelID.qwen35_2BQ8_0

    static func speechModel(
        for slot: SpeechModelKind,
        architecture: ModelHostArchitecture = .current
    ) -> SpeechModelDescriptor {
        let concreteID: String
        let publicID: String

        switch slot {
        case .english:
            concreteID = defaultEnglishSpeechModel(for: architecture)
            publicID = ModelSlot.englishSpeech
        case .multilingual:
            concreteID = defaultMultilingualSpeechModel(for: architecture)
            publicID = ModelSlot.multilingualSpeech
        }

        return speechModel(publicID: publicID, concreteID: concreteID)
    }

    static func cleanupModel(for kind: CleanupModelKind) -> CleanupModelDescriptor {
        let publicID = kind.rawValue

        switch kind {
        case .standard:
            return cleanupModel(
                publicID: publicID,
                concreteID: defaultCleanupModel
            )

        case .gemma4E2B:
            return cleanupModel(publicID: publicID, concreteID: ConcreteModelID.gemma4E2B)
        }
    }

    static func cleanupModels() -> [CleanupModelDescriptor] {
        [
            cleanupModel(for: .standard),
            cleanupModel(publicID: CleanupModelKind.gemma4E2B.rawValue, concreteID: ConcreteModelID.gemma4E2B),
        ]
    }

    private static func speechModel(publicID: String, concreteID: String) -> SpeechModelDescriptor {
        switch concreteID {
        case ConcreteModelID.whisperSmallEnglish:
            return SpeechModelDescriptor(
                publicID: publicID,
                concreteID: concreteID,
                modelID: "openai_whisper-small.en",
                repoID: nil,
                fileName: nil,
                displayName: "Whisper small.en",
                subtitle: "Fast local English speech-to-text model",
                expectedBytes: 483_000_000
            )

        case ConcreteModelID.whisperLargeV3Multilingual:
            return SpeechModelDescriptor(
                publicID: publicID,
                concreteID: concreteID,
                modelID: "openai_whisper-large-v3-v20240930_626MB",
                repoID: nil,
                fileName: nil,
                displayName: "Whisper large-v3 multilingual",
                subtitle: "Local multilingual speech-to-text model",
                expectedBytes: 627_000_000
            )

        case ConcreteModelID.whisperCppSmallEnglishQ5_1:
            return SpeechModelDescriptor(
                publicID: publicID,
                concreteID: concreteID,
                modelID: "small.en-q5_1",
                repoID: "ggerganov/whisper.cpp",
                fileName: "ggml-small.en-q5_1.bin",
                displayName: "Whisper.cpp small.en Q5_1",
                subtitle: "Intel-compatible local English speech-to-text model",
                expectedBytes: 163_000_000
            )

        case ConcreteModelID.whisperCppSmallQ5_1:
            return SpeechModelDescriptor(
                publicID: publicID,
                concreteID: concreteID,
                modelID: "small-q5_1",
                repoID: "ggerganov/whisper.cpp",
                fileName: "ggml-small-q5_1.bin",
                displayName: "Whisper.cpp small Q5_1",
                subtitle: "Intel-compatible local multilingual speech-to-text model",
                expectedBytes: 163_000_000
            )

        default:
            return speechModel(publicID: publicID, concreteID: ConcreteModelID.whisperSmallEnglish)
        }
    }

    private static func defaultEnglishSpeechModel(for architecture: ModelHostArchitecture) -> String {
        switch architecture {
        case .appleSilicon:
            return defaultEnglishSpeechModel
        case .intel:
            return ConcreteModelID.whisperCppSmallEnglishQ5_1
        }
    }

    private static func defaultMultilingualSpeechModel(for architecture: ModelHostArchitecture) -> String {
        switch architecture {
        case .appleSilicon:
            return defaultMultilingualSpeechModel
        case .intel:
            return ConcreteModelID.whisperCppSmallQ5_1
        }
    }

    private static func cleanupModel(publicID: String, concreteID: String) -> CleanupModelDescriptor {
        switch concreteID {
        case ConcreteModelID.qwen35_2BQ8_0:
            return CleanupModelDescriptor(
                publicID: publicID,
                concreteID: concreteID,
                repoID: "unsloth/Qwen3.5-2B-GGUF",
                fileName: "Qwen3.5-2B-Q8_0.gguf",
                displayName: "Qwen3.5 2B Q8_0",
                subtitle: "Local cleanup model",
                expectedBytes: 2_010_000_000,
                promptFormat: .qwen3ChatML,
                samplerConfiguration: .qwen35Cleanup,
                contextTokens: 2048,
                outputTokens: 512
            )

        case ConcreteModelID.gemma4E2B:
            return CleanupModelDescriptor(
                publicID: publicID,
                concreteID: concreteID,
                repoID: "ggml-org/gemma-4-E2B-it-GGUF",
                fileName: "gemma-4-E2B-it-Q8_0.gguf",
                displayName: "Google Gemma 4 E2B Instruct Q8_0",
                subtitle: "Higher-quality local cleanup model",
                expectedBytes: 4_970_000_000,
                promptFormat: .gemma4Turns,
                samplerConfiguration: .gemma4Cleanup,
                contextTokens: 2048,
                outputTokens: 256
            )

        default:
            preconditionFailure("Unknown cleanup model concrete ID: \(concreteID)")
        }
    }
}
