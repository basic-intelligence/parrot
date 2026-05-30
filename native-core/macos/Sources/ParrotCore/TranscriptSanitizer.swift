import Foundation

enum TranscriptSanitizer {
    static func stripNonSpeechAnnotations(from text: String) -> String {
        var output = ""
        var index = text.startIndex

        while index < text.endIndex {
            let character = text[index]
            let close: Character?
            switch character {
            case "[": close = "]"
            case "(": close = ")"
            default: close = nil
            }

            if let close,
               let closeIndex = text[text.index(after: index)...].firstIndex(of: close) {
                let inner = String(text[text.index(after: index)..<closeIndex])
                if isNonSpeechAnnotation(inner) {
                    index = text.index(after: closeIndex)
                    continue
                }
            }

            output.append(character)
            index = text.index(after: index)
        }

        return normalizeAnnotationSpacing(output)
    }

    private static func isNonSpeechAnnotation(_ value: String) -> Bool {
        if isMusicNoteOnly(value) {
            return true
        }

        return nonSpeechAnnotationLabels.contains(normalizeAnnotationLabel(value))
    }

    private static func normalizeAnnotationLabel(_ value: String) -> String {
        let trimSet = CharacterSet.whitespacesAndNewlines
            .union(.punctuationCharacters)
            .union(CharacterSet(charactersIn: "\"'“”‘’…"))
        let trimmed = value.trimmingCharacters(in: trimSet)
        let normalizedSeparators = trimmed
            .replacingOccurrences(of: "_", with: " ")
            .replacingOccurrences(of: "-", with: " ")
            .replacingOccurrences(of: "–", with: " ")
            .replacingOccurrences(of: "—", with: " ")

        return normalizedSeparators
            .split(whereSeparator: { $0.isWhitespace })
            .joined(separator: " ")
            .lowercased()
    }

    private static func isMusicNoteOnly(_ value: String) -> Bool {
        let trimmed = value.trimmingCharacters(in: .whitespacesAndNewlines)
        guard trimmed.isEmpty == false else { return false }

        var sawNote = false
        for scalar in trimmed.unicodeScalars {
            if musicNoteScalars.contains(scalar) {
                sawNote = true
                continue
            }

            if CharacterSet.whitespacesAndNewlines.contains(scalar)
                || allowedMusicNotePunctuation.contains(scalar) {
                continue
            }

            return false
        }

        return sawNote
    }

    private static func normalizeAnnotationSpacing(_ text: String) -> String {
        var cleaned = regexReplace(#"[ \t]+([.,!?;:])"#, in: text, with: "$1")
        cleaned = regexReplace(#"[ \t]{2,}"#, in: cleaned, with: " ")
        cleaned = regexReplace(#"[ \t]+\n"#, in: cleaned, with: "\n")
        cleaned = regexReplace(#"\n[ \t]+"#, in: cleaned, with: "\n")
        cleaned = regexReplace(#"\n{3,}"#, in: cleaned, with: "\n\n")
        return cleaned.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    private static func regexReplace(_ pattern: String, in text: String, with replacement: String) -> String {
        guard let regex = try? NSRegularExpression(
            pattern: pattern,
            options: [.caseInsensitive, .dotMatchesLineSeparators]
        ) else {
            return text
        }
        let range = NSRange(text.startIndex..<text.endIndex, in: text)
        return regex.stringByReplacingMatches(in: text, range: range, withTemplate: replacement)
    }

    private static let nonSpeechAnnotationLabels: Set<String> = [
        "applause",
        "background music",
        "background noise",
        "beep",
        "beeping",
        "blank audio",
        "breath",
        "breathing",
        "clapping",
        "cough",
        "coughing",
        "inaudible",
        "instrumental music",
        "laugh",
        "laughing",
        "laughter",
        "music",
        "music playing",
        "no speech",
        "noise",
        "silence",
        "silent",
        "sneeze",
        "sneezing",
        "static",
        "unintelligible",
    ]
    private static let musicNoteScalars = Set("♪♫♬♩♭♯".unicodeScalars)
    private static let allowedMusicNotePunctuation = Set(".,-–—".unicodeScalars)
}
