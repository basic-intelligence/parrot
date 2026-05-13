import ApplicationServices
import Foundation

enum FocusedTextContextReader {
    static func textBeforeInsertionPoint(
        processIdentifier: pid_t?,
        maxCharacters: Int = 120
    ) -> String? {
        guard maxCharacters > 0,
              let focusedElement = focusedElement(processIdentifier: processIdentifier),
              let selectedRange = selectedTextRange(in: focusedElement),
              selectedRange.location != kCFNotFound,
              selectedRange.location > 0 else {
            return nil
        }

        let cursorLocation = selectedRange.location
        let startLocation = max(0, cursorLocation - maxCharacters)
        let contextLength = cursorLocation - startLocation
        guard contextLength > 0 else { return nil }

        if let valueText = stringValueBeforeLocation(
            cursorLocation,
            in: focusedElement,
            maxCharacters: maxCharacters
        ) {
            return boundedSuffix(valueText, maxCharacters: maxCharacters)
        }

        let contextRange = CFRange(location: startLocation, length: contextLength)
        if let rangedText = stringForRange(contextRange, in: focusedElement),
           (rangedText as NSString).length == contextLength {
            return boundedSuffix(rangedText, maxCharacters: maxCharacters)
        }

        return nil
    }

    private static func focusedElement(processIdentifier: pid_t?) -> AXUIElement? {
        if let processIdentifier,
           let element = focusedElement(in: AXUIElementCreateApplication(processIdentifier)) {
            return element
        }

        return focusedElement(in: AXUIElementCreateSystemWide())
    }

    private static func focusedElement(in container: AXUIElement) -> AXUIElement? {
        var value: CFTypeRef?
        guard AXUIElementCopyAttributeValue(
            container,
            kAXFocusedUIElementAttribute as CFString,
            &value
        ) == .success else {
            return nil
        }

        guard let value,
              CFGetTypeID(value) == AXUIElementGetTypeID() else {
            return nil
        }

        return (value as! AXUIElement)
    }

    private static func selectedTextRange(in element: AXUIElement) -> CFRange? {
        var value: CFTypeRef?
        guard AXUIElementCopyAttributeValue(
            element,
            kAXSelectedTextRangeAttribute as CFString,
            &value
        ) == .success,
              let value,
              CFGetTypeID(value) == AXValueGetTypeID() else {
            return nil
        }

        let axValue = value as! AXValue
        guard AXValueGetType(axValue) == .cfRange else {
            return nil
        }

        var range = CFRange(location: 0, length: 0)
        guard AXValueGetValue(axValue, .cfRange, &range) else {
            return nil
        }

        return range
    }

    private static func stringForRange(_ range: CFRange, in element: AXUIElement) -> String? {
        var mutableRange = range
        guard let rangeValue = AXValueCreate(.cfRange, &mutableRange) else {
            return nil
        }

        var value: CFTypeRef?
        guard AXUIElementCopyParameterizedAttributeValue(
            element,
            kAXStringForRangeParameterizedAttribute as CFString,
            rangeValue,
            &value
        ) == .success else {
            return nil
        }

        return value as? String
    }

    private static func stringValueBeforeLocation(
        _ location: Int,
        in element: AXUIElement,
        maxCharacters: Int
    ) -> String? {
        var value: CFTypeRef?
        guard AXUIElementCopyAttributeValue(
            element,
            kAXValueAttribute as CFString,
            &value
        ) == .success else {
            return nil
        }

        let text: String?
        if let string = value as? String {
            text = string
        } else if let attributedString = value as? NSAttributedString {
            text = attributedString.string
        } else {
            text = nil
        }

        guard let text else { return nil }
        let nsText = text as NSString
        guard location > 0, location <= nsText.length else { return nil }
        let cursorLocation = location

        let startLocation = max(0, cursorLocation - maxCharacters)
        return nsText.substring(
            with: NSRange(
                location: startLocation,
                length: cursorLocation - startLocation
            )
        )
    }

    private static func boundedSuffix(_ text: String, maxCharacters: Int) -> String {
        if text.count <= maxCharacters {
            return text
        }

        return String(text.suffix(maxCharacters))
    }
}
