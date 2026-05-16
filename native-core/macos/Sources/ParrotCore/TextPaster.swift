import ApplicationServices
import AppKit
import CoreGraphics
import Foundation

struct TextPasteTarget: @unchecked Sendable {
    let processIdentifier: pid_t
    let focusedElement: AXUIElement?

    static func captureCurrent() -> TextPasteTarget? {
        guard let application = NSWorkspace.shared.frontmostApplication else {
            return nil
        }

        let processIdentifier = application.processIdentifier
        return TextPasteTarget(
            processIdentifier: processIdentifier,
            focusedElement: FocusedTextContextReader.focusedElement(processIdentifier: processIdentifier)
        )
    }
}

enum TextPasterError: Error, LocalizedError {
    case missingKeyboardPostPermission
    case couldNotCreatePasteEvent

    var errorDescription: String? {
        switch self {
        case .missingKeyboardPostPermission:
            return "Parrot needs Accessibility permission to paste into other apps. Enable Parrot or parrot-core in System Settings -> Privacy & Security -> Accessibility, then restart Parrot."
        case .couldNotCreatePasteEvent:
            return "Could not create paste keyboard event."
        }
    }
}

enum TextPaster {
    static func paste(_ text: String, target: TextPasteTarget? = nil) throws {
        try ensureCanPostKeyboardEvents()

        if let target {
            restorePasteTargetIfNeeded(target)
        }

        let contextualText = ContextualPasteFormatter.format(
            text,
            precedingContext: FocusedTextContextReader.textBeforeInsertionPoint(
                processIdentifier: target?.processIdentifier,
                focusedElement: target?.focusedElement
            )
        )

        guard let source = CGEventSource(stateID: .combinedSessionState),
              let keyDown = CGEvent(keyboardEventSource: source, virtualKey: 9, keyDown: true),
              let keyUp = CGEvent(keyboardEventSource: source, virtualKey: 9, keyDown: false) else {
            throw TextPasterError.couldNotCreatePasteEvent
        }

        let pasteboard = NSPasteboard.general
        let previousItems = clonePasteboardItems(pasteboard.pasteboardItems ?? [])

        pasteboard.clearContents()
        pasteboard.setString(contextualText, forType: .string)

        keyDown.flags = .maskCommand
        keyUp.flags = .maskCommand
        keyDown.post(tap: .cgAnnotatedSessionEventTap)
        keyUp.post(tap: .cgAnnotatedSessionEventTap)

        DispatchQueue.global(qos: .utility).asyncAfter(deadline: .now() + 0.8) {
            pasteboard.clearContents()
            pasteboard.writeObjects(previousItems)
        }
    }

    private static func ensureCanPostKeyboardEvents() throws {
        if #available(macOS 10.15, *) {
            if CGPreflightPostEventAccess() {
                return
            }
        }

        if AXIsProcessTrusted() {
            return
        }

        throw TextPasterError.missingKeyboardPostPermission
    }

    private static func restorePasteTargetIfNeeded(_ target: TextPasteTarget) {
        activateTargetAppIfNeeded(processIdentifier: target.processIdentifier)

        guard let focusedElement = target.focusedElement else {
            return
        }

        if let window = windowElement(for: focusedElement) {
            let applicationElement = AXUIElementCreateApplication(target.processIdentifier)
            _ = AXUIElementSetAttributeValue(
                applicationElement,
                kAXFocusedWindowAttribute as CFString,
                window
            )
        }

        _ = AXUIElementSetAttributeValue(
            focusedElement,
            kAXFocusedAttribute as CFString,
            kCFBooleanTrue
        )

        Thread.sleep(forTimeInterval: 0.05)
    }

    private static func activateTargetAppIfNeeded(processIdentifier: pid_t?) {
        guard let processIdentifier,
              let app = NSRunningApplication(processIdentifier: processIdentifier),
              app.isTerminated == false else { return }

        if app.isActive == false {
            _ = app.activate(options: [])
            Thread.sleep(forTimeInterval: 0.15)
        }
    }

    private static func windowElement(for element: AXUIElement) -> AXUIElement? {
        var value: CFTypeRef?
        guard AXUIElementCopyAttributeValue(
            element,
            kAXWindowAttribute as CFString,
            &value
        ) == .success,
              let value,
              CFGetTypeID(value) == AXUIElementGetTypeID() else {
            return nil
        }

        return (value as! AXUIElement)
    }

    private static func clonePasteboardItems(_ items: [NSPasteboardItem]) -> [NSPasteboardItem] {
        items.map { item in
            let copy = NSPasteboardItem()
            for type in item.types {
                if let data = item.data(forType: type) {
                    copy.setData(data, forType: type)
                } else if let string = item.string(forType: type) {
                    copy.setString(string, forType: type)
                }
            }
            return copy
        }
    }
}
