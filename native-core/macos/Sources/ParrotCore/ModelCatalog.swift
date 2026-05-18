import Foundation

enum ModelRole: String, Codable, Sendable {
    case speech
    case cleanup
}

enum ModelHostArchitecture: String, Sendable {
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

enum CleanupPromptFormat: String, Codable, Sendable {
    case qwen3ChatML = "qwen3Chatml"
    case gemma4Turns
}

struct LlamaSamplerConfiguration: Codable, Sendable {
    let topK: Int32
    let topP: Float
    let minP: Float
    let temperature: Float
    let repeatPenalty: Float
    let frequencyPenalty: Float
    let presencePenalty: Float

    static let fallback = LlamaSamplerConfiguration(
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
    static var defaultEnglishSpeechModel: String {
        speechModel(for: .english, architecture: .appleSilicon).concreteID
    }

    static var defaultMultilingualSpeechModel: String {
        speechModel(for: .multilingual, architecture: .appleSilicon).concreteID
    }

    static var defaultCleanupModel: String {
        cleanupModel(for: .standard).concreteID
    }

    static func speechModel(
        for slot: SpeechModelKind,
        architecture: ModelHostArchitecture = .current
    ) -> SpeechModelDescriptor {
        let publicID = slot == .english ? ModelSlot.englishSpeech : ModelSlot.multilingualSpeech
        guard let model = manifest.models.first(where: {
            $0.role == .speech &&
                $0.publicId == publicID &&
                $0.speechSlot == publicID &&
                $0.platforms.contains("macos") &&
                $0.architectures.contains(architecture.rawValue)
        }) else {
            preconditionFailure("Missing speech model for \(publicID) on \(architecture.rawValue)")
        }

        return SpeechModelDescriptor(
            publicID: model.publicId,
            concreteID: model.concreteId,
            modelID: model.modelId ?? "",
            repoID: model.repoId,
            fileName: model.fileName,
            displayName: model.displayName,
            subtitle: model.subtitle,
            expectedBytes: model.expectedBytes
        )
    }

    static func cleanupModel(for kind: CleanupModelKind) -> CleanupModelDescriptor {
        cleanupModel(publicID: kind.rawValue)
    }

    static func cleanupModels() -> [CleanupModelDescriptor] {
        manifest.models
            .filter { $0.role == .cleanup }
            .map { cleanupModel(from: $0) }
    }

    private static func cleanupModel(publicID: String) -> CleanupModelDescriptor {
        guard let model = manifest.models.first(where: {
            $0.role == .cleanup && $0.publicId == publicID
        }) else {
            preconditionFailure("Missing cleanup model for \(publicID)")
        }
        return cleanupModel(from: model)
    }

    private static func cleanupModel(from model: SharedModelDescriptor) -> CleanupModelDescriptor {
        guard let repoID = model.repoId,
              let fileName = model.fileName,
              let promptFormat = model.promptFormat
        else {
            preconditionFailure("Cleanup model \(model.publicId) is missing required fields")
        }

        return CleanupModelDescriptor(
            publicID: model.publicId,
            concreteID: model.concreteId,
            repoID: repoID,
            fileName: fileName,
            displayName: model.displayName,
            subtitle: model.subtitle,
            expectedBytes: model.expectedBytes,
            promptFormat: promptFormat,
            samplerConfiguration: model.sampler ?? .fallback,
            contextTokens: model.contextTokens ?? 2048,
            outputTokens: model.outputTokens ?? 512
        )
    }

    private static var manifest: ModelCatalogManifest {
        guard let manifest = try? SharedResources.decode(ModelCatalogManifest.self, relativePath: "models.json") else {
            preconditionFailure("Could not load shared model catalog.")
        }
        return manifest
    }
}

private struct ModelCatalogManifest: Decodable {
    let defaultCleanupPublicId: String
    let models: [SharedModelDescriptor]
}

private struct SharedModelDescriptor: Decodable {
    let publicId: String
    let concreteId: String
    let role: ModelRole
    let runtime: String
    let speechSlot: String?
    let platforms: [String]
    let architectures: [String]
    let repoId: String?
    let fileName: String?
    let modelId: String?
    let displayName: String
    let subtitle: String
    let expectedBytes: Int64
    let sha256: String?
    let license: String
    let promptFormat: CleanupPromptFormat?
    let sampler: LlamaSamplerConfiguration?
    let contextTokens: Int32?
    let outputTokens: Int32?
}
