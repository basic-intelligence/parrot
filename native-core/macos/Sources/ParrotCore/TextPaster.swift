import ApplicationServices
import AppKit
import CoreGraphics
import Foundation

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
    static func paste(_ text: String, targetProcessIdentifier: pid_t? = nil) throws {
        try ensureCanPostKeyboardEvents()

        activateTargetAppIfNeeded(processIdentifier: targetProcessIdentifier)

        let contextualText = ContextualPasteFormatter.format(
            text,
            precedingContext: FocusedTextContextReader.textBeforeInsertionPoint(
                processIdentifier: targetProcessIdentifier
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

    private static func activateTargetAppIfNeeded(processIdentifier: pid_t?) {
        guard let processIdentifier,
              let app = NSRunningApplication(processIdentifier: processIdentifier),
              app.isTerminated == false else { return }

        if app.isActive == false {
            _ = app.activate(options: [])
            Thread.sleep(forTimeInterval: 0.15)
        }
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
