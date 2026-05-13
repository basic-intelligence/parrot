import Foundation

enum CleanupOutputSanitizer {
    static func sanitize(_ output: String) -> String {
        var cleaned = regexReplace(#"<\|channel\>\s*thought\s*.*?<channel\|>"#, in: output)
        cleaned = regexReplace(#"<\|channel\>.*?<channel\|>"#, in: cleaned)
        cleaned = cleaned
            .replacingOccurrences(of: "<|im_start|>assistant", with: "")
            .replacingOccurrences(of: "<|im_start|>user", with: "")
            .replacingOccurrences(of: "<|im_start|>system", with: "")
            .replacingOccurrences(of: "<|im_end|>", with: "")
            .replacingOccurrences(of: "<|turn>assistant", with: "")
            .replacingOccurrences(of: "<|turn>model", with: "")
            .replacingOccurrences(of: "<|turn>user", with: "")
            .replacingOccurrences(of: "<|turn>system", with: "")
            .replacingOccurrences(of: "<turn|>", with: "")
            .replacingOccurrences(of: "<|think|>", with: "")
            .replacingOccurrences(of: "<|channel>thought", with: "")
            .replacingOccurrences(of: "<channel|>", with: "")
            .replacingOccurrences(of: "<|endoftext|>", with: "")
            .replacingOccurrences(of: "/no_think", with: "")
            .trimmingCharacters(in: .whitespacesAndNewlines)

        cleaned = regexReplace(#"<\s*think\b[^>]*>.*?<\s*/\s*think\s*>"#, in: cleaned)
        cleaned = regexReplace(#"<\s*/?\s*think\b[^>]*>"#, in: cleaned)
        cleaned = regexReplace(#"<\|channel\>\s*thought\s*.*?<channel\|>"#, in: cleaned)
        cleaned = regexReplace(#"<\|channel\>.*?<channel\|>"#, in: cleaned)
        cleaned = regexReplace(#"<\|/?(?:turn|tool|tool_call|tool_response)\|?>"#, in: cleaned)
        cleaned = cleaned.trimmingCharacters(in: .whitespacesAndNewlines)

        let prefixes = [
            "Output:",
            "Cleaned text:",
            "Cleaned:",
            "Cleaned transcript:",
            "Final:",
            "Final answer:",
            "Answer:",
            "Response:",
            "model",
            "assistant"
        ]

        var removedPrefix = true
        while removedPrefix {
            removedPrefix = false

            for prefix in prefixes {
                let lowercased = cleaned.lowercased()
                let normalizedPrefix = prefix.lowercased()
                guard lowercased.hasPrefix(normalizedPrefix) else { continue }

                if prefix == "model" || prefix == "assistant" {
                    let remainder = cleaned.dropFirst(prefix.count)
                    guard remainder.first.map({ $0 == ":" || $0.isWhitespace }) ?? true else {
                        continue
                    }
                }

                cleaned.removeFirst(prefix.count)
                cleaned = cleaned.trimmingCharacters(in: .whitespacesAndNewlines)
                removedPrefix = true
            }
        }

        cleaned = stripLeadingGenerationArtifacts(cleaned)

        if cleaned.hasPrefix("\"") && cleaned.hasSuffix("\"") && cleaned.count > 1 {
            cleaned.removeFirst()
            cleaned.removeLast()
        }

        cleaned = stripLeadingGenerationArtifacts(cleaned)

        return cleaned.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    private static func stripLeadingGenerationArtifacts(_ text: String) -> String {
        var cleaned = text.trimmingCharacters(in: .whitespacesAndNewlines)

        for _ in 0..<4 {
            let before = cleaned

            // Gemma occasionally emits stray wrapper characters before the real answer,
            // especially after thought/channel or turn-token cleanup.
            cleaned = regexReplace(
                #"^\s*(?:[>\]\)\}]+)\s*(?=\S)"#,
                in: cleaned
            )

            // Remove quote/backtick wrappers only when they appear as naked wrappers,
            // not normal punctuation inside the transcript.
            cleaned = regexReplace(
                #"^\s*(?:[`"“”'‘’]+)\s*(?=\S)"#,
                in: cleaned
            )

            cleaned = cleaned.trimmingCharacters(in: .whitespacesAndNewlines)

            if cleaned == before {
                break
            }
        }

        return cleaned
    }

    private static func regexReplace(
        _ pattern: String,
        in text: String,
        with replacement: String = ""
    ) -> String {
        guard let regex = try? NSRegularExpression(
            pattern: pattern,
            options: [.caseInsensitive, .dotMatchesLineSeparators]
        ) else {
            return text
        }

        let range = NSRange(text.startIndex..<text.endIndex, in: text)
        return regex.stringByReplacingMatches(
            in: text,
            options: [],
            range: range,
            withTemplate: replacement
        )
    }
}
