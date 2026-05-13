import CoreGraphics
import Foundation

final class HotkeyMonitor {
    var onStart: ((String) -> Void)?
    var onStop: ((String) -> Void)?
    var onError: ((String) -> Void)?
    var onCancel: (() -> Void)?

    private struct Binding {
        var id: String
        var mode: String
        var doubleTapToggle: Bool
        var requiredKeyCodes: Set<CGKeyCode>
        var chordActive = false
        var recordingToggleActive = false
        var holdStartedAtNanos: UInt64?
        var pendingDoubleTapStop = false
        var pendingDoubleTapGeneration = 0
        var suppressedUntilReleased = false
    }

    private let stateQueue = DispatchQueue(label: "in.basic.parrot.hotkey.state")
    private var bindings: [Binding] = [
        Binding(
            id: "pushToTalk",
            mode: "hold",
            doubleTapToggle: false,
            requiredKeyCodes: Set<CGKeyCode>([59, 58, 49])
        ),
        Binding(
            id: "handsFree",
            mode: "toggle",
            doubleTapToggle: false,
            requiredKeyCodes: Set<CGKeyCode>([59, 58, 55, 49])
        ),
    ]
    private var pressedKeyCodes = Set<CGKeyCode>()
    private var cancellationEnabled = false
    private var escapeCancellationKeyDown = false

    private var thread: Thread?
    private var runLoop: CFRunLoop?
    private var eventTap: CFMachPort?
    private var runLoopSource: CFRunLoopSource?
    private var canConsumeEvents = false
    private var healthTimer: DispatchSourceTimer?

    func update(pushToTalk: ShortcutSettings, handsFree: ShortcutSettings) {
        stateQueue.sync {
            var nextBindings: [Binding] = []
            if pushToTalk.enabled {
                nextBindings.append(Self.binding(id: "pushToTalk", mode: "hold", shortcut: pushToTalk))
            }
            if handsFree.enabled {
                nextBindings.append(Self.binding(id: "handsFree", mode: "toggle", shortcut: handsFree))
            }
            self.bindings = nextBindings.filter { !$0.requiredKeyCodes.isEmpty }
            self.pressedKeyCodes.removeAll()
        }
    }

    private static func binding(id: String, mode: String, shortcut: ShortcutSettings) -> Binding {
        Binding(
            id: id,
            mode: mode,
            doubleTapToggle: shortcut.doubleTapToggle,
            requiredKeyCodes: Set(shortcut.macosKeyCodes.map { CGKeyCode($0) })
        )
    }

    func setCancellationEnabled(_ enabled: Bool) {
        stateQueue.sync {
            cancellationEnabled = enabled
        }
    }

    func forceToggleOff(source: String? = nil, suppressUntilReleased: Bool = true) {
        stateQueue.async {
            for index in self.bindings.indices {
                if let source, self.bindings[index].id != source { continue }
                let activeNow = self.bindings[index].requiredKeyCodes.isSubset(of: self.pressedKeyCodes)
                if suppressUntilReleased && activeNow {
                    self.bindings[index].suppressedUntilReleased = true
                }
                self.clearPendingDoubleTapStop(at: index)
                self.bindings[index].recordingToggleActive = false
                self.bindings[index].chordActive = false
            }
        }
    }

    @discardableResult
    func start() -> Bool {
        guard thread == nil else { return true }

        guard PermissionManager.accessibilityStatus() == .granted else {
            let message = "Parrot needs Accessibility permission to consume the global shortcut and paste text. Enable Parrot Core in System Settings -> Privacy & Security -> Accessibility."
            fputs("ParrotCore HotkeyMonitor: \(message)\n", stderr)
            onError?(message)
            return false
        }

        let startupSemaphore = DispatchSemaphore(value: 0)
        let startupLock = NSLock()
        var startupSucceeded = false
        var startupMessage: String?

        let newThread = Thread { [weak self] in
            self?.eventTapThreadMain { succeeded, message in
                startupLock.lock()
                startupSucceeded = succeeded
                startupMessage = message
                startupLock.unlock()
                startupSemaphore.signal()
            }
        }
        thread = newThread
        newThread.name = "Parrot Hotkey Monitor"
        newThread.start()

        if startupSemaphore.wait(timeout: .now() + .seconds(2)) == .timedOut {
            let message = "Timed out while starting the keyboard event tap."
            fputs("ParrotCore HotkeyMonitor: \(message)\n", stderr)
            onError?(message)
            stop()
            return false
        }

        startupLock.lock()
        let succeeded = startupSucceeded
        let message = startupMessage
        startupLock.unlock()

        if !succeeded {
            if let message {
                fputs("ParrotCore HotkeyMonitor: \(message)\n", stderr)
                onError?(message)
            }
            stop()
            return false
        }

        return true
    }

    func stop() {
        stopHealthTimer()
        guard let runLoop else {
            thread = nil
            eventTap = nil
            runLoopSource = nil
            self.runLoop = nil
            return
        }

        let semaphore = DispatchSemaphore(value: 0)

        CFRunLoopPerformBlock(runLoop, CFRunLoopMode.commonModes.rawValue) { [weak self] in
            if let eventTap = self?.eventTap {
                CGEvent.tapEnable(tap: eventTap, enable: false)
            }

            if let source = self?.runLoopSource {
                CFRunLoopRemoveSource(runLoop, source, .commonModes)
            }

            CFRunLoopStop(runLoop)
            semaphore.signal()
        }

        CFRunLoopWakeUp(runLoop)

        _ = semaphore.wait(timeout: .now() + .milliseconds(750))

        stateQueue.sync {
            self.pressedKeyCodes.removeAll()
            for index in self.bindings.indices {
                self.clearPendingDoubleTapStop(at: index)
                self.bindings[index].chordActive = false
                self.bindings[index].recordingToggleActive = false
                self.bindings[index].suppressedUntilReleased = false
            }
            self.cancellationEnabled = false
            self.escapeCancellationKeyDown = false
        }

        thread = nil
        eventTap = nil
        runLoopSource = nil
        self.runLoop = nil
    }

    private func eventTapThreadMain(startup: @escaping (Bool, String?) -> Void) {
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
            callback: HotkeyMonitor.eventCallback,
            userInfo: userInfo
        ) else {
            let message: String
            if PermissionManager.inputMonitoringStatus() != .granted {
                message = "Could not create the keyboard event tap. This Mac may require Input Monitoring permission for Parrot Core."
            } else {
                message = "Could not create the keyboard event tap. Accessibility permission may be missing or macOS blocked the event tap."
            }
            startup(false, message)
            thread = nil
            return
        }

        canConsumeEvents = true
        eventTap = tap
        let source = CFMachPortCreateRunLoopSource(kCFAllocatorDefault, tap, 0)
        runLoopSource = source
        runLoop = CFRunLoopGetCurrent()
        CFRunLoopAddSource(runLoop, source, .commonModes)
        CGEvent.tapEnable(tap: tap, enable: true)
        startHealthTimer()
        startup(true, nil)
        CFRunLoopRun()
        stopHealthTimer()
        eventTap = nil
        runLoopSource = nil
        canConsumeEvents = false
        runLoop = nil
        thread = nil
    }

    private static let eventCallback: CGEventTapCallBack = { _, type, event, userInfo in
        guard let userInfo else { return Unmanaged.passUnretained(event) }
        let monitor = Unmanaged<HotkeyMonitor>.fromOpaque(userInfo).takeUnretainedValue()
        if type == .tapDisabledByTimeout || type == .tapDisabledByUserInput {
            if let eventTap = monitor.eventTap {
                CGEvent.tapEnable(tap: eventTap, enable: true)
            }
            return Unmanaged.passUnretained(event)
        }
        let shouldConsume = monitor.handle(type: type, event: event)
        if shouldConsume && monitor.canConsumeEvents {
            return nil
        }
        return Unmanaged.passUnretained(event)
    }

    private func handle(type: CGEventType, event: CGEvent) -> Bool {
        let keyCode = CGKeyCode(event.getIntegerValueField(.keyboardEventKeycode))
        let eventNanos = DispatchTime.now().uptimeNanoseconds
        var starts: [String] = []
        var stops: [String] = []
        var shouldCancel = false
        var shouldConsume = false

        stateQueue.sync {
            switch type {
            case .keyDown:
                pressedKeyCodes.insert(keyCode)
            case .keyUp:
                pressedKeyCodes.remove(keyCode)
            case .flagsChanged:
                updateModifierState(keyCode: keyCode, flags: event.flags)
            default:
                break
            }

            if keyCode == Self.escapeKeyCode {
                if escapeCancellationKeyDown {
                    shouldConsume = true
                    pressedKeyCodes.remove(Self.escapeKeyCode)
                    if type == .keyUp {
                        escapeCancellationKeyDown = false
                    }
                    return
                }

                if type == .keyDown && cancellationEnabled {
                    shouldCancel = true
                    shouldConsume = true
                    escapeCancellationKeyDown = true
                    pressedKeyCodes.remove(Self.escapeKeyCode)
                    suppressActiveBindingsUntilReleased()
                    return
                }
            }

            for index in bindings.indices {
                let wasChordActive = bindings[index].chordActive
                let activeNow = bindings[index].requiredKeyCodes.isSubset(of: pressedKeyCodes)

                if bindings[index].suppressedUntilReleased {
                    if activeNow {
                        shouldConsume = true
                        continue
                    }

                    bindings[index].suppressedUntilReleased = false
                }

                shouldConsume = shouldConsume || activeNow || wasChordActive

                if activeNow && wasChordActive == false {
                    activateBinding(
                        at: index,
                        eventNanos: eventNanos,
                        starts: &starts,
                        stops: &stops
                    )
                } else if activeNow == false && wasChordActive {
                    releaseBinding(at: index, eventNanos: eventNanos, stops: &stops)
                }
            }
        }

        if shouldCancel {
            onCancel?()
        }
        starts.forEach { onStart?($0) }
        stops.forEach { onStop?($0) }
        return shouldConsume
    }

    private func startHealthTimer() {
        stopHealthTimer()

        let timer = DispatchSource.makeTimerSource(queue: stateQueue)
        timer.schedule(deadline: .now() + .seconds(5), repeating: .seconds(5))
        timer.setEventHandler { [weak self] in
            guard let self, let eventTap = self.eventTap else { return }
            if !CGEvent.tapIsEnabled(tap: eventTap) {
                CGEvent.tapEnable(tap: eventTap, enable: true)
            }
        }
        timer.resume()
        healthTimer = timer
    }

    private func stopHealthTimer() {
        healthTimer?.cancel()
        healthTimer = nil
    }

    private func suppressActiveBindingsUntilReleased() {
        for index in bindings.indices {
            let activeNow = bindings[index].requiredKeyCodes.isSubset(of: pressedKeyCodes)
            if activeNow || bindings[index].chordActive || bindings[index].recordingToggleActive {
                bindings[index].suppressedUntilReleased = activeNow
            }

            clearPendingDoubleTapStop(at: index)
            bindings[index].chordActive = false
            bindings[index].recordingToggleActive = false
        }
    }

    private func activateBinding(
        at index: Int,
        eventNanos: UInt64,
        starts: inout [String],
        stops: inout [String]
    ) {
        if bindings[index].mode == "toggle" {
            bindings[index].chordActive = true
            bindings[index].recordingToggleActive.toggle()
            if bindings[index].recordingToggleActive {
                starts.append(bindings[index].id)
            } else {
                stops.append(bindings[index].id)
            }
            return
        }

        if bindings[index].doubleTapToggle {
            if bindings[index].recordingToggleActive {
                stops.append(bindings[index].id)
                clearPendingDoubleTapStop(at: index)
                bindings[index].recordingToggleActive = false
                bindings[index].chordActive = false
                bindings[index].suppressedUntilReleased = true
                return
            }

            if bindings[index].pendingDoubleTapStop {
                clearPendingDoubleTapStop(at: index)
                bindings[index].recordingToggleActive = true
                bindings[index].chordActive = true
                bindings[index].holdStartedAtNanos = eventNanos
                return
            }

            bindings[index].holdStartedAtNanos = eventNanos
        }

        bindings[index].chordActive = true
        starts.append(bindings[index].id)
    }

    private func releaseBinding(at index: Int, eventNanos: UInt64, stops: inout [String]) {
        bindings[index].chordActive = false

        guard bindings[index].mode != "toggle" else { return }

        if bindings[index].doubleTapToggle {
            defer { bindings[index].holdStartedAtNanos = nil }

            if bindings[index].recordingToggleActive {
                return
            }

            if let holdStartedAtNanos = bindings[index].holdStartedAtNanos,
               eventNanos - holdStartedAtNanos <= Self.doubleTapIntervalNanos {
                schedulePendingDoubleTapStop(at: index)
                return
            }
        }

        stops.append(bindings[index].id)
    }

    private func schedulePendingDoubleTapStop(at index: Int) {
        bindings[index].pendingDoubleTapStop = true
        bindings[index].pendingDoubleTapGeneration += 1
        let id = bindings[index].id
        let generation = bindings[index].pendingDoubleTapGeneration

        stateQueue.asyncAfter(deadline: .now() + Self.doubleTapInterval) { [weak self] in
            guard let self else { return }
            guard let index = self.bindings.firstIndex(where: { $0.id == id }) else { return }
            guard self.bindings[index].pendingDoubleTapStop,
                  self.bindings[index].pendingDoubleTapGeneration == generation,
                  self.bindings[index].recordingToggleActive == false else {
                return
            }

            self.bindings[index].pendingDoubleTapStop = false
            self.bindings[index].holdStartedAtNanos = nil
            self.bindings[index].chordActive = false

            let onStop = self.onStop
            DispatchQueue.global(qos: .userInitiated).async {
                onStop?(id)
            }
        }
    }

    private func clearPendingDoubleTapStop(at index: Int) {
        bindings[index].pendingDoubleTapStop = false
        bindings[index].pendingDoubleTapGeneration += 1
        bindings[index].holdStartedAtNanos = nil
    }

    private func updateModifierState(keyCode: CGKeyCode, flags: CGEventFlags) {
        let isDown: Bool
        switch keyCode {
        case 54, 55:
            isDown = flags.contains(.maskCommand)
            setModifierState([54, 55], isDown: isDown)
        case 58, 61:
            isDown = flags.contains(.maskAlternate)
            setModifierState([58, 61], isDown: isDown)
        case 59, 62:
            isDown = flags.contains(.maskControl)
            setModifierState([59, 62], isDown: isDown)
        case 56, 60:
            isDown = flags.contains(.maskShift)
            setModifierState([56, 60], isDown: isDown)
        case 63:
            isDown = flags.contains(.maskSecondaryFn)
            setModifierState([63], isDown: isDown)
        default:
            isDown = pressedKeyCodes.contains(keyCode)
            if isDown { pressedKeyCodes.insert(keyCode) } else { pressedKeyCodes.remove(keyCode) }
        }
    }

    private func setModifierState(_ keyCodes: [CGKeyCode], isDown: Bool) {
        for keyCode in keyCodes {
            if isDown { pressedKeyCodes.insert(keyCode) } else { pressedKeyCodes.remove(keyCode) }
        }
    }

    private static let escapeKeyCode: CGKeyCode = 53
    private static let doubleTapInterval: DispatchTimeInterval = .milliseconds(350)
    private static let doubleTapIntervalNanos: UInt64 = 350_000_000
}
