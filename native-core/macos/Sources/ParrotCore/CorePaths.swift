import Foundation

enum CorePaths {
    private static let lock = NSLock()
    private static var configuredPaths = defaultPaths()

    static func configure(_ paths: NativeCorePathsDTO) {
        lock.lock()
        configuredPaths = paths
        lock.unlock()
    }

    static var appDataDirectory: URL {
        url(\.appDataDir)
    }

    static var modelsDirectory: URL {
        url(\.modelsDir)
    }

    static var speechModelsDirectory: URL {
        url(\.speechModelsDir)
    }

    static var whisperCppModelsDirectory: URL {
        modelsDirectory.appendingPathComponent("whisper-cpp-models", isDirectory: true)
    }

    static var cleanupModelsDirectory: URL {
        url(\.cleanupModelsDir)
    }

    static var sharedResourcesDirectory: URL {
        url(\.sharedResourcesDir)
    }

    static var tempDirectory: URL {
        url(\.tempDir)
    }

    private static func url(_ keyPath: KeyPath<NativeCorePathsDTO, String>) -> URL {
        lock.lock()
        let value = configuredPaths[keyPath: keyPath]
        lock.unlock()
        return URL(fileURLWithPath: value, isDirectory: true)
    }

    private static func defaultPaths() -> NativeCorePathsDTO {
        let appSupport = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first!
            .appendingPathComponent("Parrot", isDirectory: true)
        let sharedResources = defaultSharedResourcesDirectory()
        return NativeCorePathsDTO(
            appDataDir: appSupport.path,
            modelsDir: appSupport.path,
            speechModelsDir: appSupport.appendingPathComponent("whisper-models", isDirectory: true).path,
            cleanupModelsDir: appSupport.appendingPathComponent("cleanup-models", isDirectory: true).path,
            resourcesDir: Bundle.main.resourceURL?.path ?? FileManager.default.currentDirectoryPath,
            sharedResourcesDir: sharedResources.path,
            tempDir: FileManager.default.temporaryDirectory.path
        )
    }

    private static func defaultSharedResourcesDirectory() -> URL {
        let cwd = URL(fileURLWithPath: FileManager.default.currentDirectoryPath, isDirectory: true)
        var sourceURL = URL(fileURLWithPath: #filePath)
        for _ in 0..<4 {
            sourceURL.deleteLastPathComponent()
        }

        var candidates = [
            cwd.appendingPathComponent("native-core/shared", isDirectory: true),
            cwd.appendingPathComponent("../shared", isDirectory: true),
            sourceURL.appendingPathComponent("shared", isDirectory: true),
        ]

        if let bundleResources = Bundle.main.resourceURL {
            candidates.append(bundleResources.appendingPathComponent("native-core/shared", isDirectory: true))
            candidates.append(bundleResources.appendingPathComponent("_up_/native-core/shared", isDirectory: true))
            candidates.append(bundleResources)
            candidates.append(bundleResources.appendingPathComponent("shared", isDirectory: true))
        }

        return candidates.first {
            FileManager.default.fileExists(atPath: $0.appendingPathComponent("languages.json").path)
                && FileManager.default.fileExists(atPath: $0.appendingPathComponent("models.json").path)
                && FileManager.default.fileExists(
                    atPath: $0
                        .appendingPathComponent("prompts", isDirectory: true)
                        .appendingPathComponent("cleanup-system-contract.md")
                        .path
                )
        } ?? sourceURL.appendingPathComponent("shared", isDirectory: true)
    }
}

enum SharedResources {
    static func url(for relativePath: String) -> URL {
        CorePaths.sharedResourcesDirectory.appendingPathComponent(relativePath)
    }

    static func text(relativePath: String, fallback: String = "") -> String {
        let resourceURL = url(for: relativePath)
        guard let text = try? String(contentsOf: resourceURL, encoding: .utf8) else {
            return fallback
        }
        return text
    }

    static func decode<T: Decodable>(_ type: T.Type, relativePath: String) throws -> T {
        let data = try Data(contentsOf: url(for: relativePath))
        return try JSONDecoder.parrot.decode(type, from: data)
    }
}
