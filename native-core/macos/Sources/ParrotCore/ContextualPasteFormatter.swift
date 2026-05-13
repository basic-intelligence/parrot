import Foundation

enum ContextualPasteFormatter {
    static func format(_ text: String, precedingContext: String?) -> String {
        guard let precedingContext,
              precedingContext.isEmpty == false,
              text.isEmpty == false else {
            return text
        }

        let currentLine = currentLineContext(from: precedingContext)
        var output = trimLeadingHorizontalWhitespaceIfWordLike(text)

        if shouldAddLeadingSpace(before: output, currentLine: currentLine) {
            output = " " + output
        }

        if shouldCapitalize(output, after: currentLine) {
            output = capitalizeFirstLetter(output)
        }

        return output
    }

    private static func currentLineContext(from text: String) -> String {
        guard let lastLineBreak = text.lastIndex(where: isLineBreak) else {
            return text
        }

        return String(text[text.index(after: lastLineBreak)...])
    }

    private static func isLineBreak(_ character: Character) -> Bool {
        character.unicodeScalars.contains { scalar in
            CharacterSet.newlines.contains(scalar)
        }
    }

    private static func shouldAddLeadingSpace(before text: String, currentLine: String) -> Bool {
        guard let trailingCharacter = currentLine.last,
              trailingCharacter.isWhitespace == false,
              startsWithWordLikeToken(text),
              shouldSeparateAfter(trailingCharacter, currentLine: currentLine) else {
            return false
        }

        return true
    }

    private static func shouldCapitalize(_ text: String, after currentLine: String) -> Bool {
        guard startsWithLowercaseWord(text) else { return false }

        let trimmedLine = currentLine.trimmingCharacters(in: .whitespacesAndNewlines)
        guard trimmedLine.isEmpty == false else { return true }

        guard let semantic = lastSemanticCharacter(in: trimmedLine) else { return false }
        return semantic == "." || semantic == "!" || semantic == "?"
    }

    private static func trimLeadingHorizontalWhitespaceIfWordLike(_ text: String) -> String {
        let trimmed = text.drop { character in
            character == " " || character == "\t"
        }
        let candidate = String(trimmed)
        return startsWithWordLikeToken(candidate) ? candidate : text
    }

    private static func startsWithWordLikeToken(_ text: String) -> Bool {
        guard let firstCharacter = text.first else { return false }
        return isLetterOrDigit(firstCharacter)
    }

    private static func startsWithLowercaseWord(_ text: String) -> Bool {
        let trimmed = text.drop { character in
            character == " " || character == "\t"
        }
        guard let firstCharacter = trimmed.first else { return false }

        let prefix = trimmed.prefix { character in
            character.unicodeScalars.allSatisfy { scalar in
                CharacterSet.letters.contains(scalar)
            }
        }

        guard prefix.count >= 2 else { return false }
        return String(prefix) == String(prefix).lowercased()
            && firstCharacter.lowercased() == String(firstCharacter)
    }

    private static func capitalizeFirstLetter(_ text: String) -> String {
        guard let index = text.firstIndex(where: isLetter) else {
            return text
        }

        var output = text
        output.replaceSubrange(index...index, with: String(output[index]).uppercased())
        return output
    }

    private static func shouldSeparateAfter(_ character: Character, currentLine: String) -> Bool {
        if isLetterOrDigit(character) {
            return true
        }

        if character.unicodeScalars.contains(where: { scalar in
            closingQuoteSeparators.contains(scalar)
        }) {
            return closingQuoteLooksClosed(in: currentLine)
        }

        return character.unicodeScalars.contains { scalar in
            trailingSeparators.contains(scalar)
        }
    }

    private static func closingQuoteLooksClosed(in currentLine: String) -> Bool {
        guard currentLine.isEmpty == false else { return false }

        let lineBeforeTrailingQuote = String(currentLine.dropLast())
        guard let semantic = lastSemanticCharacter(in: lineBeforeTrailingQuote) else {
            return false
        }

        return isLetterOrDigit(semantic)
            || semantic == "."
            || semantic == "!"
            || semantic == "?"
    }

    private static func lastSemanticCharacter(in text: String) -> Character? {
        text.reversed().first { character in
            character.isWhitespace == false && semanticWrappers.contains(character) == false
        }
    }

    private static func isLetter(_ character: Character) -> Bool {
        character.unicodeScalars.contains { scalar in
            CharacterSet.letters.contains(scalar)
        }
    }

    private static func isLetterOrDigit(_ character: Character) -> Bool {
        character.unicodeScalars.contains { scalar in
            CharacterSet.letters.contains(scalar) || CharacterSet.decimalDigits.contains(scalar)
        }
    }

    private static let trailingSeparators = Set(".!,?;:)]}\"”’»›".unicodeScalars)
    private static let closingQuoteSeparators = Set("\"”’»›".unicodeScalars)
    private static let semanticWrappers = Set<Character>([")", "]", "}", "\"", "”", "’", "»", "›"])
}
