import Foundation

struct JSONRequest: Decodable {
    let id: String
    let method: String
    let payload: JSONValue
}

struct JSONResponse: Encodable {
    let id: String
    let ok: Bool
    let payload: JSONValue?
    let error: String?
}

struct JSONEvent: Encodable {
    let event: String
    let payload: JSONValue
}

enum JSONValue: Codable, Sendable {
    case null
    case bool(Bool)
    case number(Double)
    case string(String)
    case array([JSONValue])
    case object([String: JSONValue])

    init(from decoder: Decoder) throws {
        let container = try decoder.singleValueContainer()
        if container.decodeNil() { self = .null }
        else if let value = try? container.decode(Bool.self) { self = .bool(value) }
        else if let value = try? container.decode(Double.self) { self = .number(value) }
        else if let value = try? container.decode(String.self) { self = .string(value) }
        else if let value = try? container.decode([JSONValue].self) { self = .array(value) }
        else { self = .object(try container.decode([String: JSONValue].self)) }
    }

    func encode(to encoder: Encoder) throws {
        var container = encoder.singleValueContainer()
        switch self {
        case .null: try container.encodeNil()
        case .bool(let value): try container.encode(value)
        case .number(let value): try container.encode(value)
        case .string(let value): try container.encode(value)
        case .array(let value): try container.encode(value)
        case .object(let value): try container.encode(value)
        }
    }

    subscript(key: String) -> JSONValue? {
        if case .object(let dict) = self { return dict[key] }
        return nil
    }

    var stringValue: String? { if case .string(let s) = self { s } else { nil } }
    var boolValue: Bool? { if case .bool(let b) = self { b } else { nil } }
    var numberValue: Double? { if case .number(let n) = self { n } else { nil } }
    var objectValue: [String: JSONValue]? { if case .object(let o) = self { o } else { nil } }
}

extension Encodable {
    func jsonValue() throws -> JSONValue {
        let data = try JSONEncoder.parrot.encode(self)
        return try JSONDecoder.parrot.decode(JSONValue.self, from: data)
    }
}

extension JSONEncoder {
    static let parrot: JSONEncoder = {
        let encoder = JSONEncoder()
        encoder.dateEncodingStrategy = .iso8601
        return encoder
    }()
}

extension JSONDecoder {
    static let parrot: JSONDecoder = {
        let decoder = JSONDecoder()
        decoder.dateDecodingStrategy = .iso8601
        return decoder
    }()
}

struct AppSettings: Codable, Sendable {
    var selectedInputUid: String?
    var pushToTalkShortcut: ShortcutSettings
    var handsFreeShortcut: ShortcutSettings
    var dictationLanguageMode: DictationLanguageMode
    var dictationLanguageCode: String?
    var cleanupModelId: String
    var cleanupEnabled: Bool
    var cleanupPrompt: String
    var dictionaryEntries: [DictionaryEntry]
    var playSounds: Bool
    var historyEnabled: Bool
    var launchAtLogin: Bool
    var onboardingCompleted: Bool
    var inputMonitoringPermissionShownInOnboarding: Bool
    var pasteIntoRecordingStartWindow: Bool

    init(
        selectedInputUid: String?,
        pushToTalkShortcut: ShortcutSettings,
        handsFreeShortcut: ShortcutSettings,
        dictationLanguageMode: DictationLanguageMode,
        dictationLanguageCode: String?,
        cleanupModelId: String,
        cleanupEnabled: Bool,
        cleanupPrompt: String,
        dictionaryEntries: [DictionaryEntry],
        playSounds: Bool,
        historyEnabled: Bool,
        launchAtLogin: Bool,
        onboardingCompleted: Bool,
        inputMonitoringPermissionShownInOnboarding: Bool,
        pasteIntoRecordingStartWindow: Bool = false
    ) {
        self.selectedInputUid = selectedInputUid
        self.pushToTalkShortcut = pushToTalkShortcut
        self.handsFreeShortcut = handsFreeShortcut
        self.dictationLanguageMode = dictationLanguageMode
        self.dictationLanguageCode = dictationLanguageCode
        self.cleanupModelId = cleanupModelId
        self.cleanupEnabled = cleanupEnabled
        self.cleanupPrompt = cleanupPrompt
        self.dictionaryEntries = dictionaryEntries
        self.playSounds = playSounds
        self.historyEnabled = historyEnabled
        self.launchAtLogin = launchAtLogin
        self.onboardingCompleted = onboardingCompleted
        self.inputMonitoringPermissionShownInOnboarding = inputMonitoringPermissionShownInOnboarding
        self.pasteIntoRecordingStartWindow = pasteIntoRecordingStartWindow
    }

    enum CodingKeys: String, CodingKey {
        case selectedInputUid
        case pushToTalkShortcut
        case handsFreeShortcut
        case dictationLanguageMode
        case dictationLanguageCode
        case cleanupModelId
        case cleanupEnabled
        case cleanupPrompt
        case dictionaryEntries
        case playSounds
        case historyEnabled
        case launchAtLogin
        case onboardingCompleted
        case inputMonitoringPermissionShownInOnboarding
        case pasteIntoRecordingStartWindow
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)

        selectedInputUid = try container.decodeIfPresent(String.self, forKey: .selectedInputUid)
        pushToTalkShortcut = try container.decode(ShortcutSettings.self, forKey: .pushToTalkShortcut)
        handsFreeShortcut = try container.decode(ShortcutSettings.self, forKey: .handsFreeShortcut)
        dictationLanguageMode = try container.decode(DictationLanguageMode.self, forKey: .dictationLanguageMode)
        dictationLanguageCode = try container.decodeIfPresent(String.self, forKey: .dictationLanguageCode)
        cleanupModelId = try container.decodeIfPresent(String.self, forKey: .cleanupModelId) ?? "cleanup"
        cleanupEnabled = try container.decode(Bool.self, forKey: .cleanupEnabled)
        cleanupPrompt = try container.decodeIfPresent(String.self, forKey: .cleanupPrompt) ?? ""
        dictionaryEntries = try container.decodeIfPresent([DictionaryEntry].self, forKey: .dictionaryEntries) ?? []
        playSounds = try container.decode(Bool.self, forKey: .playSounds)
        historyEnabled = try container.decode(Bool.self, forKey: .historyEnabled)
        launchAtLogin = try container.decode(Bool.self, forKey: .launchAtLogin)
        onboardingCompleted = try container.decodeIfPresent(Bool.self, forKey: .onboardingCompleted) ?? false
        inputMonitoringPermissionShownInOnboarding = try container.decodeIfPresent(
            Bool.self,
            forKey: .inputMonitoringPermissionShownInOnboarding
        ) ?? false
        pasteIntoRecordingStartWindow = try container.decodeIfPresent(
            Bool.self,
            forKey: .pasteIntoRecordingStartWindow
        ) ?? false
    }
}

struct DictionaryEntry: Codable, Sendable, Equatable {
    var id: String
    var term: String
}

struct ShortcutSettings: Codable, Sendable {
    var displayName: String
    var macosKeyCodes: [UInt16]
    var mode: String
    var enabled: Bool
    var doubleTapToggle: Bool

    init(
        displayName: String,
        macosKeyCodes: [UInt16],
        mode: String,
        enabled: Bool = true,
        doubleTapToggle: Bool = false
    ) {
        self.displayName = displayName
        self.macosKeyCodes = macosKeyCodes
        self.mode = mode
        self.enabled = enabled
        self.doubleTapToggle = doubleTapToggle
    }

    enum CodingKeys: String, CodingKey {
        case displayName, macosKeyCodes, mode, enabled, doubleTapToggle
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        displayName = try container.decode(String.self, forKey: .displayName)
        macosKeyCodes = try container.decode([UInt16].self, forKey: .macosKeyCodes)
        mode = try container.decode(String.self, forKey: .mode)
        enabled = try container.decodeIfPresent(Bool.self, forKey: .enabled) ?? true
        doubleTapToggle = try container.decodeIfPresent(Bool.self, forKey: .doubleTapToggle) ?? false
    }
}

struct AudioDeviceDTO: Codable, Sendable {
    let uid: String
    let name: String
    let isDefault: Bool
}

struct ModelStatusDTO: Codable, Sendable {
    let id: String
    let role: ModelRole
    let displayName: String
    let subtitle: String
    let expectedBytes: Int64
    let localBytes: Int64
    let progressBytes: Int64
    let progressTotalBytes: Int64
    let downloaded: Bool
    let downloading: Bool
    let required: Bool
    let error: String?
}

struct PermissionSnapshotDTO: Codable, Sendable {
    let microphone: PermissionState
    let accessibility: PermissionState
    let inputMonitoring: PermissionState
    let allGranted: Bool
}

struct RecordingResultDTO: Codable, Sendable {
    let raw: String
    let cleaned: String
    let audioDurationSeconds: Double
}
