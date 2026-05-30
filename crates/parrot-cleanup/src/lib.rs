use regex::RegexBuilder;

pub fn strip_non_speech_annotations(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut index = 0;

    while index < input.len() {
        let Some(character) = input[index..].chars().next() else {
            break;
        };

        let close = match character {
            '[' => Some(']'),
            '(' => Some(')'),
            _ => None,
        };

        if let Some(close) = close {
            let open_end = index + character.len_utf8();
            if let Some(relative_close) = input[open_end..].find(close) {
                let close_index = open_end + relative_close;
                let inner = &input[open_end..close_index];
                if is_non_speech_annotation(inner) {
                    index = close_index + close.len_utf8();
                    continue;
                }
            }
        }

        output.push(character);
        index += character.len_utf8();
    }

    normalize_annotation_spacing(&output)
}

fn is_non_speech_annotation(value: &str) -> bool {
    if is_music_note_only(value) {
        return true;
    }

    matches!(
        normalize_annotation_label(value).as_str(),
        "applause"
            | "background music"
            | "background noise"
            | "beep"
            | "beeping"
            | "blank audio"
            | "breath"
            | "breathing"
            | "clapping"
            | "cough"
            | "coughing"
            | "inaudible"
            | "instrumental music"
            | "laugh"
            | "laughing"
            | "laughter"
            | "music"
            | "music playing"
            | "no speech"
            | "noise"
            | "silence"
            | "silent"
            | "sneeze"
            | "sneezing"
            | "static"
            | "unintelligible"
    )
}

fn normalize_annotation_label(value: &str) -> String {
    value
        .trim()
        .trim_matches(|character: char| {
            character.is_ascii_punctuation() || matches!(character, '“' | '”' | '‘' | '’' | '…')
        })
        .chars()
        .map(|character| match character {
            '_' | '-' | '–' | '—' => ' ',
            character => character,
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn is_music_note_only(value: &str) -> bool {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return false;
    }

    let mut saw_note = false;
    for character in trimmed.chars() {
        if matches!(character, '♪' | '♫' | '♬' | '♩' | '♭' | '♯') {
            saw_note = true;
            continue;
        }

        if character.is_whitespace() || matches!(character, '.' | ',' | '-' | '–' | '—') {
            continue;
        }

        return false;
    }

    saw_note
}

fn normalize_annotation_spacing(text: &str) -> String {
    let mut cleaned = regex_replace(r"[ \t]+([.,!?;:])", text, "$1");
    cleaned = regex_replace(r"[ \t]{2,}", &cleaned, " ");
    cleaned = regex_replace(r"[ \t]+\n", &cleaned, "\n");
    cleaned = regex_replace(r"\n[ \t]+", &cleaned, "\n");
    cleaned = regex_replace(r"\n{3,}", &cleaned, "\n\n");
    cleaned.trim().to_string()
}

pub fn sanitize(output: &str) -> String {
    let mut cleaned = regex_replace(r"<\|channel\>\s*thought\s*.*?<channel\|>", output, "");
    cleaned = regex_replace(r"<\|channel\>.*?<channel\|>", &cleaned, "");
    cleaned = cleaned
        .replace("<|im_start|>assistant", "")
        .replace("<|im_start|>user", "")
        .replace("<|im_start|>system", "")
        .replace("<|im_end|>", "")
        .replace("<|turn>assistant", "")
        .replace("<|turn>model", "")
        .replace("<|turn>user", "")
        .replace("<|turn>system", "")
        .replace("<turn|>", "")
        .replace("<|think|>", "")
        .replace("<|channel>thought", "")
        .replace("<channel|>", "")
        .replace("<|endoftext|>", "")
        .replace("/no_think", "")
        .trim()
        .to_string();

    cleaned = regex_replace(r"<\s*think\b[^>]*>.*?<\s*/\s*think\s*>", &cleaned, "");
    cleaned = regex_replace(r"<\s*/?\s*think\b[^>]*>", &cleaned, "");
    cleaned = regex_replace(r"<\|channel\>\s*thought\s*.*?<channel\|>", &cleaned, "");
    cleaned = regex_replace(r"<\|channel\>.*?<channel\|>", &cleaned, "");
    cleaned = regex_replace(
        r"<\|/?(?:turn|tool|tool_call|tool_response)\|?>",
        &cleaned,
        "",
    );
    cleaned = cleaned.trim().to_string();

    cleaned = strip_leading_output_prefixes(&cleaned);
    cleaned = strip_leading_generation_artifacts(&cleaned);
    cleaned = strip_leading_plain_text_reasoning(&cleaned);
    cleaned = strip_leading_output_prefixes(&cleaned);

    if cleaned.is_empty() {
        return String::new();
    }

    cleaned = strip_leading_generation_artifacts(&cleaned);

    if cleaned.starts_with('"') && cleaned.ends_with('"') && cleaned.chars().count() > 1 {
        cleaned = cleaned[1..cleaned.len() - 1].to_string();
    }

    cleaned = strip_leading_plain_text_reasoning(&cleaned);
    cleaned = strip_leading_output_prefixes(&cleaned);

    strip_leading_generation_artifacts(&cleaned)
        .trim()
        .to_string()
}

fn strip_leading_output_prefixes(text: &str) -> String {
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
        "assistant",
    ];

    let mut cleaned = text.trim().to_string();
    let mut removed_prefix = true;
    while removed_prefix {
        removed_prefix = false;
        for prefix in prefixes {
            if !cleaned.to_lowercase().starts_with(&prefix.to_lowercase()) {
                continue;
            }

            if prefix == "model" || prefix == "assistant" {
                let remainder = cleaned[prefix.len()..].chars().next();
                let allowed = match remainder {
                    None | Some(':') => true,
                    Some(character) => character.is_whitespace(),
                };
                if !allowed {
                    continue;
                }
            }

            cleaned = cleaned[prefix.len()..].trim().to_string();
            removed_prefix = true;
        }
    }

    cleaned
}

fn strip_leading_plain_text_reasoning(text: &str) -> String {
    let cleaned = text.trim();
    let Some(regex) =
        RegexBuilder::new(r"^\s*(thinking process|thought process|reasoning|analysis)\s*:\s*")
            .case_insensitive(true)
            .build()
            .ok()
    else {
        return cleaned.to_string();
    };

    let Some(captures) = regex.captures(cleaned) else {
        return cleaned.to_string();
    };
    let Some(full_match) = captures.get(0) else {
        return cleaned.to_string();
    };
    let Some(header_match) = captures.get(1) else {
        return cleaned.to_string();
    };

    let body = &cleaned[full_match.end()..];
    if let Some(final_output) = final_output_from_reasoning_body(body) {
        return final_output.trim().to_string();
    }

    if is_likely_plain_text_reasoning(header_match.as_str(), body) {
        return String::new();
    }

    cleaned.to_string()
}

fn final_output_from_reasoning_body(body: &str) -> Option<String> {
    let regex = RegexBuilder::new(
        r"^\s*(?:Final answer|Final|Cleaned transcript|Cleaned text|Cleaned|Output|Answer|Response)\s*:",
    )
    .case_insensitive(true)
    .multi_line(true)
    .build()
    .ok()?;

    regex
        .find(body)
        .map(|matched| body[matched.start()..].to_string())
}

fn is_likely_plain_text_reasoning(header: &str, body: &str) -> bool {
    let trimmed_body = body.trim();
    if trimmed_body.is_empty() {
        return true;
    }

    if regex_contains(r"^\s*\d+\.\s+", trimmed_body) {
        return true;
    }

    if regex_contains(r"^\s*[*-]\s+\*\*?[A-Za-z][^:\n]{0,64}:\*\*?", trimmed_body) {
        return true;
    }

    let lowercased = trimmed_body.to_lowercase();
    let model_process_signals = [
        "analyze the request",
        "input:",
        "task:",
        "constraint",
        "raw transcript",
        "cleanup rules",
        "final transformed transcript",
        "return only",
        "do not drop content",
        "the user asked",
        "the instructions",
    ];

    if model_process_signals
        .iter()
        .any(|signal| lowercased.contains(signal))
    {
        return true;
    }

    let normalized_header = header.to_lowercase();
    if normalized_header == "thinking process" || normalized_header == "thought process" {
        return trimmed_body.contains('\n') || trimmed_body.contains("**");
    }

    false
}

fn strip_leading_generation_artifacts(text: &str) -> String {
    let mut cleaned = text.trim().to_string();

    for _ in 0..4 {
        let before = cleaned.clone();
        cleaned = trim_leading_chars(&cleaned, &['>', ']', ')', '}']);
        cleaned = trim_leading_chars(&cleaned, &['`', '"', '“', '”', '\'', '‘', '’']);
        cleaned = cleaned.trim().to_string();
        if cleaned == before {
            break;
        }
    }

    cleaned
}

fn trim_leading_chars(text: &str, chars: &[char]) -> String {
    let trimmed = text.trim_start();
    let Some(first) = trimmed.chars().next() else {
        return String::new();
    };
    if chars.contains(&first) {
        return trimmed[first.len_utf8()..].trim_start().to_string();
    }
    trimmed.to_string()
}

fn regex_replace(pattern: &str, text: &str, replacement: &str) -> String {
    RegexBuilder::new(pattern)
        .case_insensitive(true)
        .dot_matches_new_line(true)
        .build()
        .map(|regex| regex.replace_all(text, replacement).to_string())
        .unwrap_or_else(|_| text.to_string())
}

fn regex_contains(pattern: &str, text: &str) -> bool {
    RegexBuilder::new(pattern)
        .case_insensitive(true)
        .multi_line(true)
        .build()
        .map(|regex| regex.is_match(text))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, Deserialize)]
    struct Fixture {
        name: String,
        input: String,
        expected: String,
    }

    #[test]
    fn matches_shared_sanitizer_fixtures() {
        let fixtures: Vec<Fixture> = serde_json::from_str(include_str!(
            "../../../native-core/shared/test-fixtures/cleanup-sanitizer.json"
        ))
        .unwrap();

        for fixture in fixtures {
            assert_eq!(
                sanitize(&fixture.input),
                fixture.expected,
                "{}",
                fixture.name
            );
        }
    }

    #[test]
    fn matches_shared_transcript_sanitizer_fixtures() {
        let fixtures: Vec<Fixture> = serde_json::from_str(include_str!(
            "../../../native-core/shared/test-fixtures/transcript-sanitizer.json"
        ))
        .unwrap();

        for fixture in fixtures {
            assert_eq!(
                strip_non_speech_annotations(&fixture.input),
                fixture.expected,
                "{}",
                fixture.name
            );
        }
    }
}
