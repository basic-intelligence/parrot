import AppKit
import Foundation

actor CoreService {
    private var settings: AppSettings = AppSettings(
        selectedInputUid: nil,
        pushToTalkShortcut: ShortcutSettings(
            displayName: "Fn",
            macosKeyCodes: [63],
            mode: "hold",
            enabled: true,
            doubleTapToggle: false
        ),
        handsFreeShortcut: ShortcutSettings(
            displayName: "Control + Space",
            macosKeyCodes: [59, 49],
            mode: "toggle",
            enabled: true,
            doubleTapToggle: false
        ),
        dictationLanguageMode: .english,
        dictationLanguageCode: nil,
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
    private let recorder = AudioRecorder()
    private let pipeline = ModelPipeline()
    private let hotkeyMonitor = HotkeyMonitor()
    private var hotkeyRecording = false
    private var activeHotkeySessionID: UUID?
    private var hotkeyProcessingTask: Task<Void, Never>?
    private var speechWarmTask: Task<Void, Never>?
    private var activeHotkeySource: String?
    private var pasteTarget: TextPasteTarget?

    init() {
        let monitor = hotkeyMonitor
        monitor.onStart = { [weak self] source in
            Task { await self?.handleHotkeyStart(source: source) }
        }
        monitor.onStop = { [weak self] source in
            Task { await self?.handleHotkeyStop(source: source) }
        }
        monitor.onError = { [weak self] message in
            Task { await self?.handleHotkeyMonitorError(message) }
        }
        monitor.onCancel = { [weak self] in
            Task { await self?.handleHotkeyCancel() }
        }
    }

    func shutdown() async {
        hotkeyProcessingTask?.cancel()
        speechWarmTask?.cancel()
        hotkeyMonitor.stop()
        if hotkeyRecording {
            _ = await recorder.stop()
        }
        await pipeline.shutdown()
    }

    func handle(_ request: JSONRequest) async throws -> JSONValue {
        switch request.method {
        case "initialize":
            if let languageCatalogValue = request.payload["languageCatalog"] {
                let languageCatalog = try decode([LanguageCatalogEntry].self, from: languageCatalogValue)
                LanguageCatalog.configure(languageCatalog)
            }
            if let settingsValue = request.payload["settings"] {
                settings = try decode(AppSettings.self, from: settingsValue)
                recorder.preferredInputUID = settings.selectedInputUid
                hotkeyMonitor.update(
                    pushToTalk: settings.pushToTalkShortcut,
                    handsFree: settings.handsFreeShortcut
                )
            }
            let cleanupPrompt = request.payload["prompts"]?["cleanupTranscript"]?.stringValue ?? ""
            let debugCleanupFailures = request.payload["debugCleanupFailures"]?.boolValue ?? false
            await pipeline.configure(
                cleanupPrompt: cleanupPrompt,
                debugCleanupFailures: debugCleanupFailures
            )
            return JSONValue.object(["status": .string("initialized")])

        case "permissionStatuses":
            return try PermissionManager.snapshot().jsonValue()

        case "requestPermission":
            let kind = request.payload["kind"]?.stringValue ?? ""
            let openSettings = request.payload["openSettings"]?.boolValue ?? false
            switch kind {
            case "microphone":
                _ = await PermissionManager.requestMicrophone(openSettings: openSettings)
            case "accessibility":
                _ = PermissionManager.requestAccessibility(openSettings: openSettings)
            case "inputMonitoring":
                _ = PermissionManager.requestInputMonitoring(openSettings: openSettings)
            default:
                throw CoreError.unknownMethod("requestPermission:\(kind)")
            }
            return try PermissionManager.snapshot().jsonValue()

        case "updateSettings":
            if let settingsValue = request.payload["settings"] {
                settings = try decode(AppSettings.self, from: settingsValue)
                recorder.preferredInputUID = settings.selectedInputUid
                hotkeyMonitor.update(
                    pushToTalk: settings.pushToTalkShortcut,
                    handsFree: settings.handsFreeShortcut
                )
                recorder.resetForRouteChange()
            }
            return try settings.jsonValue()

        case "warmModels":
            await pipeline.warmModels(settings: settings)
            return JSONValue.object(["status": .string("warmed")])

        case "modelStatuses":
            return try await pipeline.modelStatuses(settings: settings).jsonValue()

        case "downloadModel":
            let kind = request.payload["kind"]?.stringValue ?? ""
            return try await pipeline.startDownload(kind: kind, settings: settings).jsonValue()

        case "deleteModel":
            let kind = request.payload["kind"]?.stringValue ?? ""
            return try await pipeline.deleteModel(kind: kind, settings: settings).jsonValue()

        case "listAudioDevices":
            return try InputDeviceManager.listInputDevices().map {
                AudioDeviceDTO(uid: $0.uid, name: $0.name, isDefault: $0.isDefault)
            }.jsonValue()

        case "startRecording":
            do {
                guard PermissionManager.microphoneStatus() == .granted else {
                    throw CoreError.microphonePermissionRequired
                }
                recorder.preferredInputUID = settings.selectedInputUid
                try recorder.start()

                let warmSettings = settings
                speechWarmTask?.cancel()
                speechWarmTask = Task { [pipeline] in
                    await pipeline.warmSpeechModel(settings: warmSettings)
                }

                return JSONValue.object(["status": .string("recording")])
            } catch {
                SoundFeedback.playFailure(enabled: settings.playSounds)
                throw error
            }

        case "stopRecording":
            do {
                let result = try await finishRecording()
                SoundFeedback.playSuccess(enabled: settings.playSounds)
                return try result.jsonValue()
            } catch {
                SoundFeedback.playFailure(enabled: settings.playSounds)
                throw error
            }

        case "startHotkeyMonitor":
            hotkeyMonitor.update(
                pushToTalk: settings.pushToTalkShortcut,
                handsFree: settings.handsFreeShortcut
            )
            guard hotkeyMonitor.start() else {
                throw CoreError.hotkeyMonitorUnavailable
            }
            return JSONValue.object(["status": .string("hotkey-monitoring")])

        case "stopHotkeyMonitor":
            hotkeyMonitor.stop()
            return JSONValue.object(["status": .string("hotkey-stopped")])

        case "captureShortcut":
            let target = request.payload["target"]?.stringValue ?? ""

            let mode: String
            switch target {
            case "pushToTalkShortcut":
                mode = "hold"
            case "handsFreeShortcut":
                mode = "toggle"
            default:
                throw CoreError.unknownMethod("captureShortcut:\(target)")
            }

            hotkeyMonitor.stop()
            let shortcut = try await ShortcutCapture.capture(mode: mode)
            return try shortcut.jsonValue()

        default:
            throw CoreError.unknownMethod(request.method)
        }
    }

    private func finishRecording(checkCancellation: Bool = false) async throws -> RecordingResultDTO {
        let currentSettings = settings
        let rawSamples = await recorder.stop()
        if checkCancellation { try Task.checkCancellation() }

        let samples = SpeechActivityTrimmer.trimForDictation(rawSamples)
        guard !samples.isEmpty else {
            throw ModelPipelineError.emptyTranscription
        }

        let transcription = try await pipeline.transcribe(samples: samples, settings: currentSettings)
        if checkCancellation { try Task.checkCancellation() }

        let cleaned = try await pipeline.cleanup(
            rawText: transcription.text,
            settings: currentSettings,
            language: transcription.language
        )
        if checkCancellation { try Task.checkCancellation() }

        return RecordingResultDTO(
            raw: transcription.text,
            cleaned: cleaned,
            audioDurationSeconds: Double(rawSamples.count) / 16_000.0
        )
    }

    private func handleHotkeyStart(source: String) async {
        if activeHotkeySessionID != nil {
            if activeHotkeySource != source { hotkeyMonitor.forceToggleOff(source: source) }
            return
        }
        pasteTarget = settings.pasteIntoRecordingStartWindow
            ? TextPasteTarget.captureCurrent()
            : nil
        do {
            recorder.preferredInputUID = settings.selectedInputUid
            try recorder.start()

            let warmSettings = settings
            speechWarmTask?.cancel()
            speechWarmTask = Task { [pipeline] in
                await pipeline.warmSpeechModel(settings: warmSettings)
            }

            activeHotkeySessionID = UUID()
            hotkeyRecording = true
            activeHotkeySource = source
            hotkeyMonitor.setCancellationEnabled(true)
            emitEvent("parrot:recording-started", payload: ["kind": .string("dictation")])
        } catch {
            activeHotkeySessionID = nil
            pasteTarget = nil
            activeHotkeySource = nil
            hotkeyMonitor.setCancellationEnabled(false)
            hotkeyMonitor.forceToggleOff()
            SoundFeedback.playFailure(enabled: settings.playSounds)
            emitEvent("parrot:recording-failed", payload: ["error": .string(error.localizedDescription)])
        }
    }

    private func handleHotkeyStop(source: String) async {
        guard hotkeyRecording else { return }
        if let activeHotkeySource, activeHotkeySource != source { return }
        guard let sessionID = activeHotkeySessionID else {
            activeHotkeySource = nil
            pasteTarget = nil
            hotkeyRecording = false
            hotkeyMonitor.setCancellationEnabled(false)
            return
        }
        activeHotkeySource = nil
        let target = pasteTarget
        pasteTarget = nil
        hotkeyRecording = false
        emitEvent("parrot:recording-processing", payload: ["kind": .string("dictation")])

        hotkeyProcessingTask?.cancel()
        hotkeyProcessingTask = Task { [weak self] in
            guard let self else { return }
            await self.finishHotkeyRecording(
                sessionID: sessionID,
                pasteTarget: target
            )
        }
    }

    private func finishHotkeyRecording(
        sessionID: UUID,
        pasteTarget: TextPasteTarget?
    ) async {
        defer { clearHotkeySessionIfCurrent(sessionID) }

        do {
            let result = try await finishRecording(checkCancellation: true)
            guard isHotkeySessionCurrent(sessionID), !Task.isCancelled else { return }

            if result.cleaned.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty == false {
                try TextPaster.paste(
                    result.cleaned,
                    target: pasteTarget
                )
                SoundFeedback.playSuccess(enabled: settings.playSounds)
            }
            guard isHotkeySessionCurrent(sessionID), !Task.isCancelled else { return }

            var payload = try result.jsonValue().objectValue ?? [:]
            payload["kind"] = .string("dictation")
            emitEvent("parrot:recording-finished", payload: payload)
        } catch is CancellationError {
            // The Escape path emits parrot:recording-cancelled and owns user-facing feedback.
        } catch {
            guard isHotkeySessionCurrent(sessionID) else { return }
            SoundFeedback.playFailure(enabled: settings.playSounds)
            emitEvent("parrot:recording-failed", payload: ["error": .string(error.localizedDescription)])
        }
    }

    private func handleHotkeyCancel() async {
        guard activeHotkeySessionID != nil || hotkeyRecording || hotkeyProcessingTask != nil else { return }

        let wasRecording = hotkeyRecording
        activeHotkeySessionID = nil
        hotkeyRecording = false
        activeHotkeySource = nil
        pasteTarget = nil

        hotkeyProcessingTask?.cancel()
        speechWarmTask?.cancel()
        hotkeyProcessingTask = nil
        hotkeyMonitor.forceToggleOff()
        hotkeyMonitor.setCancellationEnabled(false)

        if wasRecording {
            _ = await recorder.stop()
        }

        SoundFeedback.playCancel(enabled: settings.playSounds)
        emitEvent("parrot:recording-cancelled", payload: ["kind": .string("dictation")])
    }

    private func isHotkeySessionCurrent(_ sessionID: UUID) -> Bool {
        activeHotkeySessionID == sessionID
    }

    private func clearHotkeySessionIfCurrent(_ sessionID: UUID) {
        guard activeHotkeySessionID == sessionID else { return }
        activeHotkeySessionID = nil
        hotkeyProcessingTask = nil
        activeHotkeySource = nil
        pasteTarget = nil
        hotkeyMonitor.setCancellationEnabled(false)
    }

    private func handleHotkeyMonitorError(_ message: String) async {
        emitEvent("parrot:hotkey-monitor-failed", payload: [
            "error": .string(message)
        ])
    }

    private func emitEvent(_ name: String, payload: [String: JSONValue]) {
        JSONLineWriter.shared.event(name, payload: .object(payload))
    }

    private func decode<T: Decodable>(_ type: T.Type, from value: JSONValue) throws -> T {
        let data = try JSONEncoder.parrot.encode(value)
        return try JSONDecoder.parrot.decode(T.self, from: data)
    }
}

private enum SoundFeedback {
    static func playSuccess(enabled: Bool) {
        playSystemSound(named: "Pop", enabled: enabled)
    }

    static func playFailure(enabled: Bool) {
        playSystemSound(named: "Basso", enabled: enabled)
    }

    static func playCancel(enabled: Bool) {
        playSystemSound(named: "Submarine", enabled: enabled)
    }

    private static func playSystemSound(named name: String, enabled: Bool) {
        guard enabled else { return }
        _ = NSSound(named: NSSound.Name(name))?.play()
    }
}

enum CoreError: Error, LocalizedError {
    case unknownMethod(String)
    case hotkeyMonitorUnavailable
    case microphonePermissionRequired

    var errorDescription: String? {
        switch self {
        case .unknownMethod(let method):
            return "Unknown native-core method: \(method)"
        case .hotkeyMonitorUnavailable:
            return "Could not start the shortcut monitor. Enable Accessibility for Parrot Core. Some Macs may also require Input Monitoring."
        case .microphonePermissionRequired:
            return "Microphone permission is required for Parrot Core."
        }
    }
}
