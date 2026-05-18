import Foundation

struct AssembledCleanupPrompt {
    let prompt: String
    let maxOutputTokens: Int32
}

enum CleanupPromptAssembler {
    private static let defaultCleanupRules = "Clean dictated text for punctuation, formatting, self-corrections, and readability."

    private static var cleanupSystemContract: String {
        SharedResources.text(relativePath: "prompts/cleanup-system-contract.md")
            .trimmingCharacters(in: .whitespacesAndNewlines)
    }

    static func assemble(
        promptFormat: CleanupPromptFormat,
        cleanupRules: String,
        dictionaryEntries: [DictionaryEntry],
        transcript: String,
        language: DictationLanguageMetadata,
        defaultOutputTokens: Int32
    ) -> AssembledCleanupPrompt {
        let prompt: String
        switch promptFormat {
        case .qwen3ChatML:
            prompt = qwen3CleanupChatPrompt(
                cleanupRules: cleanupRules,
                dictionaryEntries: dictionaryEntries,
                transcript: transcript,
                language: language
            )
        case .gemma4Turns:
            prompt = gemma4CleanupChatPrompt(
                cleanupRules: cleanupRules,
                dictionaryEntries: dictionaryEntries,
                transcript: transcript,
                language: language
            )
        }

        return AssembledCleanupPrompt(
            prompt: prompt,
            maxOutputTokens: cleanupOutputTokenBudget(
                for: transcript,
                defaultLimit: defaultOutputTokens
            )
        )
    }

    static func cleanupOutputTokenBudget(
        for transcript: String,
        defaultLimit: Int32
    ) -> Int32 {
        let wordCount = transcript.split { character in
            character.isWhitespace || character.isNewline
        }.count

        let characterFallback = max(1, transcript.count / 4)
        let contentEstimate = max(wordCount * 3, characterFallback)
        let budget = max(96, min(Int(defaultLimit), contentEstimate + 64))

        return Int32(budget)
    }

    static func qwen3CleanupChatPrompt(
        cleanupRules: String,
        dictionaryEntries: [DictionaryEntry],
        transcript: String,
        language: DictationLanguageMetadata
    ) -> String {
        let systemPrompt = cleanupSystemPrompt(dictionaryEntries: dictionaryEntries)
        let userPrompt = cleanupUserPrompt(
            cleanupRules: cleanupRules,
            transcript: transcript,
            language: language
        )

        return renderPromptTemplate(
            SharedResources.text(relativePath: "prompts/formats/qwen3-chatml.txt"),
            values: [
                "system_prompt": systemPrompt,
                "user_prompt": userPrompt,
            ]
        )
    }

    static func gemma4CleanupChatPrompt(
        cleanupRules: String,
        dictionaryEntries: [DictionaryEntry],
        transcript: String,
        language: DictationLanguageMetadata
    ) -> String {
        let systemPrompt = cleanupSystemPrompt(dictionaryEntries: dictionaryEntries)
        let userPrompt = cleanupUserPrompt(
            cleanupRules: cleanupRules,
            transcript: transcript,
            language: language
        )

        return renderPromptTemplate(
            SharedResources.text(relativePath: "prompts/formats/gemma4-turns.txt"),
            values: [
                "system_prompt": systemPrompt,
                "user_prompt": userPrompt,
            ]
        )
    }

    static func cleanupUserPrompt(
        cleanupRules: String,
        transcript: String,
        language: DictationLanguageMetadata
    ) -> String {
        let rules = escapePromptDelimitedText(cleanupRules)
        return renderPromptTemplate(
            SharedResources.text(relativePath: "prompts/cleanup-user-template.md"),
            values: [
                "cleanup_rules": rules.isEmpty ? defaultCleanupRules : rules,
                "dictation_language_xml": language.xmlElement,
                "raw_transcript": escapePromptDelimitedText(transcript),
            ]
        )
    }

    static func renderPromptTemplate(_ template: String, values: [String: String]) -> String {
        values.reduce(template) { rendered, pair in
            rendered.replacingOccurrences(of: "{{ \(pair.key) }}", with: pair.value)
        }
    }

    static func cleanupSystemPrompt(dictionaryEntries: [DictionaryEntry]) -> String {
        let dictionarySection = dictionaryTermsSystemSection(dictionaryEntries)
        guard !dictionarySection.isEmpty else { return cleanupSystemContract }
        return cleanupSystemContract + "\n\n" + dictionarySection
    }

    static func dictionaryTermsSystemSection(_ dictionaryEntries: [DictionaryEntry]) -> String {
        var terms: [String] = []
        var seenTerms = Set<String>()

        for entry in dictionaryEntries {
            let term = escapePromptDelimitedText(sanitizedDictionaryValue(entry.term))
            guard !term.isEmpty else { continue }

            if seenTerms.insert(term.lowercased()).inserted {
                terms.append(term)
            }

            if terms.count >= 200 {
                break
            }
        }

        guard !terms.isEmpty else {
            return ""
        }

        return renderPromptTemplate(
            SharedResources.text(relativePath: "prompts/dictionary-system-section.md"),
            values: [
                "dictionary_terms": terms.map { "- \($0)" }.joined(separator: "\n"),
            ]
        )
    }

    static func escapePromptDelimitedText(_ value: String) -> String {
        value
            .replacingOccurrences(of: "\0", with: "")
            .replacingOccurrences(of: "<|im_start|>", with: "")
            .replacingOccurrences(of: "<|im_end|>", with: "")
            .replacingOccurrences(of: "<|turn>system", with: "")
            .replacingOccurrences(of: "<|turn>user", with: "")
            .replacingOccurrences(of: "<|turn>model", with: "")
            .replacingOccurrences(of: "<turn|>", with: "")
            .replacingOccurrences(of: "&", with: "&amp;")
            .replacingOccurrences(of: "<", with: "&lt;")
            .replacingOccurrences(of: ">", with: "&gt;")
            .trimmingCharacters(in: .whitespacesAndNewlines)
    }

    static func sanitizedDictionaryValue(_ value: String) -> String {
        value
            .replacingOccurrences(of: "<|im_start|>", with: "")
            .replacingOccurrences(of: "<|im_end|>", with: "")
            .replacingOccurrences(of: "<|turn>system", with: "")
            .replacingOccurrences(of: "<|turn>user", with: "")
            .replacingOccurrences(of: "<|turn>model", with: "")
            .replacingOccurrences(of: "<turn|>", with: "")
            .replacingOccurrences(of: "\n", with: " ")
            .replacingOccurrences(of: "\r", with: " ")
            .trimmingCharacters(in: .whitespacesAndNewlines)
    }
}
