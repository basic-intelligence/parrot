import CoreGraphics
import Foundation

enum ShortcutCaptureError: Error, LocalizedError {
    case cancelled
    case unavailable
    case invalid(String)

    var errorDescription: String? {
        switch self {
        case .cancelled:
            return "Shortcut capture cancelled."
        case .unavailable:
            return "Could not start shortcut capture. Check Accessibility permission. Some Macs may also require Input Monitoring."
        case .invalid(let message):
            return message
        }
    }
}

enum ShortcutCapture {
    static func capture(mode: String) async throws -> ShortcutSettings {
        try await ShortcutCaptureSession(mode: mode).run()
    }
}

private final class ShortcutCaptureSession {
    private struct KeyInfo {
        let label: String
        let isFunctionKey: Bool
    }

    private struct ModifierInfo {
        let code: CGKeyCode
        let label: String
    }

    private let mode: String
    private let lock = NSLock()
    private var continuation: CheckedContinuation<ShortcutSettings, Error>?
    private var completed = false
    private var thread: Thread?
    private var runLoop: CFRunLoop?
    private var eventTap: CFMachPort?
    private var runLoopSource: CFRunLoopSource?
    private var currentModifiers = Set<CGKeyCode>()
    private var maxModifierCountDuringChord = 0
    private var sawNonModifierDuringChord = false

    init(mode: String) {
        self.mode = mode
    }

    func run() async throws -> ShortcutSettings {
        try await withCheckedThrowingContinuation { continuation in
            self.lock.lock()
            self.continuation = continuation
            self.lock.unlock()
            self.start()
        }
    }

    private func start() {
        let newThread = Thread { [weak self] in
            self?.eventTapThreadMain()
        }
        newThread.name = "Parrot Shortcut Capture"

        lock.lock()
        thread = newThread
        lock.unlock()

        newThread.start()
    }

    private func eventTapThreadMain() {
        let mask = CGEventMask(
            (1 << CGEventType.keyDown.rawValue) |
            (1 << CGEventType.keyUp.rawValue) |
            (1 << CGEventType.flagsChanged.rawValue)
        )
        let userInfo = Unmanaged.passUnretained(self).toOpaque()

        guard let tap = CGEvent.tapCreate(
            tap: .cgSessionEventTap,
            place: .headInsertEventTap,
            options: .defaultTap,
            eventsOfInterest: mask,
            callback: ShortcutCaptureSession.eventCallback,
            userInfo: userInfo
        ) else {
            complete(.failure(ShortcutCaptureError.unavailable))
            return
        }

        let source = CFMachPortCreateRunLoopSource(kCFAllocatorDefault, tap, 0)
        let currentRunLoop = CFRunLoopGetCurrent()

        lock.lock()
        eventTap = tap
        runLoopSource = source
        runLoop = currentRunLoop
        lock.unlock()

        CFRunLoopAddSource(currentRunLoop, source, .commonModes)
        CGEvent.tapEnable(tap: tap, enable: true)
        CFRunLoopRun()

        lock.lock()
        eventTap = nil
        runLoopSource = nil
        runLoop = nil
        thread = nil
        lock.unlock()
    }

    private static let eventCallback: CGEventTapCallBack = { _, type, event, userInfo in
        guard let userInfo else { return Unmanaged.passUnretained(event) }
        let session = Unmanaged<ShortcutCaptureSession>.fromOpaque(userInfo).takeUnretainedValue()
        return session.handle(type: type, event: event) ? nil : Unmanaged.passUnretained(event)
    }

    private func handle(type: CGEventType, event: CGEvent) -> Bool {
        lock.lock()
        let alreadyCompleted = completed
        lock.unlock()

        if alreadyCompleted {
            return true
        }

        if type == .tapDisabledByTimeout || type == .tapDisabledByUserInput {
            if let tap = currentEventTap() {
                CGEvent.tapEnable(tap: tap, enable: true)
            }
            return false
        }

        switch type {
        case .flagsChanged:
            handleFlagsChanged(event)
        case .keyDown:
            handleKeyDown(event)
        case .keyUp:
            break
        default:
            return false
        }

        return true
    }

    private func handleFlagsChanged(_ event: CGEvent) {
        let previousModifiers = currentModifiers
        let nextModifiers = Self.modifierCodes(from: event.flags)

        currentModifiers = nextModifiers
        maxModifierCountDuringChord = max(maxModifierCountDuringChord, nextModifiers.count)

        if mode == "hold",
           previousModifiers.count == 1,
           nextModifiers.isEmpty,
           maxModifierCountDuringChord == 1,
           sawNonModifierDuringChord == false {
            saveShortcut(modifiers: previousModifiers, keyCode: nil, keyInfo: nil)
            return
        }

        if nextModifiers.isEmpty {
            resetChordTracking()
        }
    }

    private func handleKeyDown(_ event: CGEvent) {
        let keyCode = CGKeyCode(event.getIntegerValueField(.keyboardEventKeycode))

        if keyCode == Self.escapeKeyCode {
            complete(.failure(ShortcutCaptureError.cancelled))
            return
        }

        if Self.normalizedModifierKeyCode(keyCode) != nil {
            return
        }

        sawNonModifierDuringChord = true

        guard let keyInfo = Self.nonModifierKeys[keyCode] else {
            complete(.failure(ShortcutCaptureError.invalid(
                "That key is not supported yet (key code \(keyCode)). Try a letter, number, Space, arrow key, or function key."
            )))
            return
        }

        let modifiers = Self.modifierCodes(from: event.flags).union(currentModifiers)
        currentModifiers = modifiers

        if modifiers.isEmpty && keyInfo.isFunctionKey == false {
            complete(.failure(ShortcutCaptureError.invalid(
                "Use at least one modifier, like Control, Option, Shift, Command, or Fn."
            )))
            return
        }

        saveShortcut(modifiers: modifiers, keyCode: keyCode, keyInfo: keyInfo)
    }

    private func saveShortcut(
        modifiers: Set<CGKeyCode>,
        keyCode: CGKeyCode?,
        keyInfo: KeyInfo?
    ) {
        let orderedModifiers = Self.orderedModifiers(from: modifiers)
        guard keyCode != nil || orderedModifiers.count == 1 else {
            complete(.failure(ShortcutCaptureError.invalid(
                "Choose a single modifier, or hold a modifier and press another key."
            )))
            return
        }

        var displayParts = orderedModifiers.map(\.label)
        var keyCodes = orderedModifiers.map(\.code)
        if let keyInfo, let keyCode {
            displayParts.append(keyInfo.label)
            keyCodes.append(keyCode)
        }

        complete(.success(ShortcutSettings(
            displayName: displayParts.joined(separator: " + "),
            macosKeyCodes: keyCodes.map { UInt16($0) },
            mode: mode
        )))
    }

    private func resetChordTracking() {
        maxModifierCountDuringChord = 0
        sawNonModifierDuringChord = false
    }

    private func complete(_ result: Result<ShortcutSettings, Error>) {
        lock.lock()
        guard completed == false else {
            lock.unlock()
            return
        }
        completed = true
        lock.unlock()

        DispatchQueue.global(qos: .userInitiated).async { [self] in
            finish(result)
        }
    }

    private func finish(_ result: Result<ShortcutSettings, Error>) {
        let continuationToResume: CheckedContinuation<ShortcutSettings, Error>?
        let runLoopToStop: CFRunLoop?

        lock.lock()
        continuationToResume = continuation
        continuation = nil
        runLoopToStop = runLoop
        lock.unlock()

        if let runLoopToStop {
            let semaphore = DispatchSemaphore(value: 0)

            CFRunLoopPerformBlock(runLoopToStop, CFRunLoopMode.commonModes.rawValue) { [weak self] in
                if let eventTap = self?.eventTap {
                    CGEvent.tapEnable(tap: eventTap, enable: false)
                }

                if let source = self?.runLoopSource {
                    CFRunLoopRemoveSource(runLoopToStop, source, .commonModes)
                }

                CFRunLoopStop(runLoopToStop)
                semaphore.signal()
            }

            CFRunLoopWakeUp(runLoopToStop)
            _ = semaphore.wait(timeout: .now() + .milliseconds(750))
        }

        lock.lock()
        eventTap = nil
        runLoopSource = nil
        runLoop = nil
        thread = nil
        lock.unlock()

        continuationToResume?.resume(with: result)
    }

    private func currentEventTap() -> CFMachPort? {
        lock.lock()
        let tap = eventTap
        lock.unlock()
        return tap
    }

    private static func modifierCodes(from flags: CGEventFlags) -> Set<CGKeyCode> {
        var codes = Set<CGKeyCode>()
        if flags.contains(.maskCommand) { codes.insert(55) }
        if flags.contains(.maskControl) { codes.insert(59) }
        if flags.contains(.maskAlternate) { codes.insert(58) }
        if flags.contains(.maskShift) { codes.insert(56) }
        if flags.contains(.maskSecondaryFn) { codes.insert(63) }
        return codes
    }

    private static func normalizedModifierKeyCode(_ keyCode: CGKeyCode) -> CGKeyCode? {
        switch keyCode {
        case 54, 55:
            return 55
        case 58, 61:
            return 58
        case 59, 62:
            return 59
        case 56, 60:
            return 56
        case 63:
            return 63
        default:
            return nil
        }
    }

    private static func orderedModifiers(from codes: Set<CGKeyCode>) -> [ModifierInfo] {
        modifierDisplayOrder.filter { codes.contains($0.code) }
    }

    private static let escapeKeyCode: CGKeyCode = 53
    private static let modifierDisplayOrder: [ModifierInfo] = [
        ModifierInfo(code: 55, label: "Command"),
        ModifierInfo(code: 59, label: "Control"),
        ModifierInfo(code: 58, label: "Option"),
        ModifierInfo(code: 56, label: "Shift"),
        ModifierInfo(code: 63, label: "Fn"),
    ]
    private static let nonModifierKeys: [CGKeyCode: KeyInfo] = [
        0: KeyInfo(label: "A", isFunctionKey: false),
        1: KeyInfo(label: "S", isFunctionKey: false),
        2: KeyInfo(label: "D", isFunctionKey: false),
        3: KeyInfo(label: "F", isFunctionKey: false),
        4: KeyInfo(label: "H", isFunctionKey: false),
        5: KeyInfo(label: "G", isFunctionKey: false),
        6: KeyInfo(label: "Z", isFunctionKey: false),
        7: KeyInfo(label: "X", isFunctionKey: false),
        8: KeyInfo(label: "C", isFunctionKey: false),
        9: KeyInfo(label: "V", isFunctionKey: false),
        11: KeyInfo(label: "B", isFunctionKey: false),
        12: KeyInfo(label: "Q", isFunctionKey: false),
        13: KeyInfo(label: "W", isFunctionKey: false),
        14: KeyInfo(label: "E", isFunctionKey: false),
        15: KeyInfo(label: "R", isFunctionKey: false),
        16: KeyInfo(label: "Y", isFunctionKey: false),
        17: KeyInfo(label: "T", isFunctionKey: false),
        18: KeyInfo(label: "1", isFunctionKey: false),
        19: KeyInfo(label: "2", isFunctionKey: false),
        20: KeyInfo(label: "3", isFunctionKey: false),
        21: KeyInfo(label: "4", isFunctionKey: false),
        22: KeyInfo(label: "6", isFunctionKey: false),
        23: KeyInfo(label: "5", isFunctionKey: false),
        24: KeyInfo(label: "=", isFunctionKey: false),
        25: KeyInfo(label: "9", isFunctionKey: false),
        26: KeyInfo(label: "7", isFunctionKey: false),
        27: KeyInfo(label: "-", isFunctionKey: false),
        28: KeyInfo(label: "8", isFunctionKey: false),
        29: KeyInfo(label: "0", isFunctionKey: false),
        30: KeyInfo(label: "]", isFunctionKey: false),
        31: KeyInfo(label: "O", isFunctionKey: false),
        32: KeyInfo(label: "U", isFunctionKey: false),
        33: KeyInfo(label: "[", isFunctionKey: false),
        34: KeyInfo(label: "I", isFunctionKey: false),
        35: KeyInfo(label: "P", isFunctionKey: false),
        36: KeyInfo(label: "Return", isFunctionKey: false),
        37: KeyInfo(label: "L", isFunctionKey: false),
        38: KeyInfo(label: "J", isFunctionKey: false),
        39: KeyInfo(label: "'", isFunctionKey: false),
        40: KeyInfo(label: "K", isFunctionKey: false),
        41: KeyInfo(label: ";", isFunctionKey: false),
        42: KeyInfo(label: "\\", isFunctionKey: false),
        43: KeyInfo(label: ",", isFunctionKey: false),
        44: KeyInfo(label: "/", isFunctionKey: false),
        45: KeyInfo(label: "N", isFunctionKey: false),
        46: KeyInfo(label: "M", isFunctionKey: false),
        47: KeyInfo(label: ".", isFunctionKey: false),
        48: KeyInfo(label: "Tab", isFunctionKey: false),
        49: KeyInfo(label: "Space", isFunctionKey: false),
        50: KeyInfo(label: "`", isFunctionKey: false),
        51: KeyInfo(label: "Delete", isFunctionKey: false),
        96: KeyInfo(label: "F5", isFunctionKey: true),
        97: KeyInfo(label: "F6", isFunctionKey: true),
        98: KeyInfo(label: "F7", isFunctionKey: true),
        99: KeyInfo(label: "F3", isFunctionKey: true),
        100: KeyInfo(label: "F8", isFunctionKey: true),
        101: KeyInfo(label: "F9", isFunctionKey: true),
        103: KeyInfo(label: "F11", isFunctionKey: true),
        109: KeyInfo(label: "F10", isFunctionKey: true),
        111: KeyInfo(label: "F12", isFunctionKey: true),
        118: KeyInfo(label: "F4", isFunctionKey: true),
        120: KeyInfo(label: "F2", isFunctionKey: true),
        122: KeyInfo(label: "F1", isFunctionKey: true),
        123: KeyInfo(label: "Left Arrow", isFunctionKey: false),
        124: KeyInfo(label: "Right Arrow", isFunctionKey: false),
        125: KeyInfo(label: "Down Arrow", isFunctionKey: false),
        126: KeyInfo(label: "Up Arrow", isFunctionKey: false),
    ]
}
