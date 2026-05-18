use regex::RegexBuilder;

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

    cleaned = strip_leading_generation_artifacts(&cleaned);

    if cleaned.starts_with('"') && cleaned.ends_with('"') && cleaned.chars().count() > 1 {
        cleaned = cleaned[1..cleaned.len() - 1].to_string();
    }

    strip_leading_generation_artifacts(&cleaned)
        .trim()
        .to_string()
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
}
