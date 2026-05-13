import Foundation

enum DictationLanguageMode: String, Codable, Hashable, Sendable {
    case english
    case detect
    case specific
}

enum SpeechModelKind: String, Codable, CaseIterable, Sendable {
    case english = "speech"
    case multilingual = "speech-multilingual"
}

enum CleanupModelKind: String, Codable, CaseIterable, Sendable {
    case standard = "cleanup"
    case gemma4E2B = "cleanup-gemma-4-e2b"
}

struct LanguageCatalogEntry: Codable, Equatable, Sendable {
    var code: String
    var speechCode: String
    var name: String
    var nativeName: String
    var variantOf: String? = nil
}

enum LanguageCatalog {
    private static let lock = NSLock()
    private static var entriesByCode: [String: LanguageCatalogEntry] = [:]

    static func configure(_ entries: [LanguageCatalogEntry]) {
        lock.lock()
        defer { lock.unlock() }

        entriesByCode = Dictionary(
            uniqueKeysWithValues: entries.map { entry in
                (normalize(entry.code), entry)
            }
        )
    }

    static func entry(for code: String?) -> LanguageCatalogEntry? {
        guard let normalized = normalizedCode(code) else { return nil }

        lock.lock()
        defer { lock.unlock() }

        return entriesByCode[normalized]
    }

    static func canonicalCode(for code: String?) -> String? {
        guard let normalized = normalizedCode(code) else { return nil }
        return entry(for: normalized)?.code ?? normalized
    }

    static func speechCode(for code: String?) -> String? {
        guard let normalized = normalizedCode(code) else { return nil }
        let speechCode = entry(for: normalized)?.speechCode.trimmingCharacters(in: .whitespacesAndNewlines)
        guard let speechCode, !speechCode.isEmpty else { return normalized }
        return speechCode.lowercased()
    }

    private static func normalizedCode(_ code: String?) -> String? {
        guard let normalized = code?
            .trimmingCharacters(in: .whitespacesAndNewlines)
            .lowercased(),
            !normalized.isEmpty
        else { return nil }

        return normalized
    }

    private static func normalize(_ code: String) -> String {
        code.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
    }
}

struct DictationLanguageMetadata: Equatable, Sendable {
    var mode: String
    var code: String?
    var locale: String?
    var name: String?

    var xmlElement: String {
        var attributes = ["mode=\"\(escapeXML(mode))\""]
        if let code, !code.isEmpty {
            attributes.append("code=\"\(escapeXML(code))\"")
        }
        if let locale, !locale.isEmpty {
            attributes.append("locale=\"\(escapeXML(locale))\"")
        }
        if let name, !name.isEmpty {
            attributes.append("name=\"\(escapeXML(name))\"")
        }
        return "<dictation_language \(attributes.joined(separator: " ")) />"
    }

    static let unknown = DictationLanguageMetadata(
        mode: "detected",
        code: nil,
        locale: nil,
        name: "unknown"
    )

    private func escapeXML(_ value: String) -> String {
        value
            .replacingOccurrences(of: "&", with: "&amp;")
            .replacingOccurrences(of: "\"", with: "&quot;")
            .replacingOccurrences(of: "<", with: "&lt;")
            .replacingOccurrences(of: ">", with: "&gt;")
    }
}

enum DictationRouting {
    static func speechModelKind(for settings: AppSettings) -> SpeechModelKind {
        usesEnglishRoute(for: settings) ? .english : .multilingual
    }

    static func cleanupModelKind(for settings: AppSettings) -> CleanupModelKind {
        CleanupModelKind(rawValue: settings.cleanupModelId) ?? .standard
    }

    static func usesEnglishRoute(for settings: AppSettings) -> Bool {
        switch settings.dictationLanguageMode {
        case .english:
            return true
        case .specific:
            return speechLanguageCode(settings.dictationLanguageCode) == "en"
        case .detect:
            return false
        }
    }

    static func speechLanguageCode(_ code: String?) -> String? {
        LanguageCatalog.speechCode(for: code)
    }

    static func decodeLanguageCode(for settings: AppSettings) -> String? {
        switch settings.dictationLanguageMode {
        case .english:
            return "en"
        case .specific:
            return speechLanguageCode(settings.dictationLanguageCode)
        case .detect:
            return nil
        }
    }

    static func shouldDetectLanguage(for settings: AppSettings) -> Bool {
        settings.dictationLanguageMode == .detect
    }

    static func selectedLanguageMetadata(for settings: AppSettings) -> DictationLanguageMetadata {
        switch settings.dictationLanguageMode {
        case .english:
            return DictationLanguageMetadata(mode: "selected", code: "en", locale: nil, name: "English")
        case .specific:
            let code = decodeLanguageCode(for: settings)
            let locale = selectedLocaleCode(for: settings, speechCode: code)
            return DictationLanguageMetadata(
                mode: "selected",
                code: code,
                locale: locale,
                name: nil
            )
        case .detect:
            return .unknown
        }
    }

    static func detectedLanguageMetadata(code: String?) -> DictationLanguageMetadata {
        let normalized = code?.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        guard let normalized, !normalized.isEmpty else { return .unknown }
        return DictationLanguageMetadata(
            mode: "detected",
            code: normalized,
            locale: nil,
            name: nil
        )
    }

    private static func selectedLocaleCode(for settings: AppSettings, speechCode: String?) -> String? {
        guard let selectedCode = LanguageCatalog.canonicalCode(for: settings.dictationLanguageCode),
              let speechCode,
              selectedCode.lowercased() != speechCode.lowercased()
        else { return nil }

        return selectedCode
    }
}
