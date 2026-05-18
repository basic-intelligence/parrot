pub fn format_contextual_paste(text: &str, preceding_context: Option<&str>) -> String {
    let Some(preceding_context) = preceding_context else {
        return text.to_string();
    };
    if preceding_context.is_empty() || text.is_empty() {
        return text.to_string();
    }

    let current_line = current_line_context(preceding_context);
    let mut output = trim_leading_horizontal_whitespace_if_word_like(text);

    if should_add_leading_space(&output, current_line) {
        output = format!(" {output}");
    }

    if should_capitalize(&output, current_line) {
        output = capitalize_first_letter(&output);
    }

    output
}

fn current_line_context(text: &str) -> &str {
    text.rsplit_once(|character| matches!(character, '\n' | '\r'))
        .map(|(_, current)| current)
        .unwrap_or(text)
}

fn should_add_leading_space(text: &str, current_line: &str) -> bool {
    let Some(trailing_character) = current_line.chars().next_back() else {
        return false;
    };

    !trailing_character.is_whitespace()
        && starts_with_word_like_token(text)
        && should_separate_after(trailing_character, current_line)
}

fn should_capitalize(text: &str, current_line: &str) -> bool {
    if !starts_with_lowercase_word(text) {
        return false;
    }

    let trimmed_line = current_line.trim();
    if trimmed_line.is_empty() {
        return true;
    }

    matches!(
        last_semantic_character(trimmed_line),
        Some('.') | Some('!') | Some('?')
    )
}

fn trim_leading_horizontal_whitespace_if_word_like(text: &str) -> String {
    let candidate = text.trim_start_matches([' ', '\t']);
    if starts_with_word_like_token(candidate) {
        candidate.to_string()
    } else {
        text.to_string()
    }
}

fn starts_with_word_like_token(text: &str) -> bool {
    text.chars().next().map(is_letter_or_digit).unwrap_or(false)
}

fn starts_with_lowercase_word(text: &str) -> bool {
    let trimmed = text.trim_start_matches([' ', '\t']);
    let Some(first_character) = trimmed.chars().next() else {
        return false;
    };

    let prefix = trimmed
        .chars()
        .take_while(|character| character.is_alphabetic())
        .collect::<String>();

    prefix.chars().count() >= 2
        && prefix == prefix.to_lowercase()
        && first_character.to_lowercase().to_string() == first_character.to_string()
}

fn capitalize_first_letter(text: &str) -> String {
    let Some((index, character)) = text
        .char_indices()
        .find(|(_, character)| character.is_alphabetic())
    else {
        return text.to_string();
    };

    let mut output = String::new();
    output.push_str(&text[..index]);
    output.push_str(&character.to_uppercase().to_string());
    output.push_str(&text[index + character.len_utf8()..]);
    output
}

fn should_separate_after(character: char, current_line: &str) -> bool {
    if is_letter_or_digit(character) {
        return true;
    }

    if CLOSING_QUOTE_SEPARATORS.contains(character) {
        return closing_quote_looks_closed(current_line);
    }

    TRAILING_SEPARATORS.contains(character)
}

fn closing_quote_looks_closed(current_line: &str) -> bool {
    let line_before_trailing_quote = current_line
        .char_indices()
        .next_back()
        .map(|(index, _)| &current_line[..index])
        .unwrap_or("");
    let Some(semantic) = last_semantic_character(line_before_trailing_quote) else {
        return false;
    };

    is_letter_or_digit(semantic) || matches!(semantic, '.' | '!' | '?')
}

fn last_semantic_character(text: &str) -> Option<char> {
    text.chars()
        .rev()
        .find(|character| !character.is_whitespace() && !SEMANTIC_WRAPPERS.contains(*character))
}

fn is_letter_or_digit(character: char) -> bool {
    character.is_alphabetic() || character.is_numeric()
}

const TRAILING_SEPARATORS: &str = ".!,?;:)]}\"”’»›";
const CLOSING_QUOTE_SEPARATORS: &str = "\"”’»›";
const SEMANTIC_WRAPPERS: &str = ")]}\"”’»›";

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Fixture {
        name: String,
        input: String,
        preceding_context: Option<String>,
        expected: String,
    }

    #[test]
    fn matches_shared_contextual_paste_fixtures() {
        let fixtures: Vec<Fixture> = serde_json::from_str(include_str!(
            "../../../native-core/shared/test-fixtures/contextual-paste.json"
        ))
        .unwrap();

        for fixture in fixtures {
            assert_eq!(
                format_contextual_paste(&fixture.input, fixture.preceding_context.as_deref()),
                fixture.expected,
                "{}",
                fixture.name
            );
        }
    }
}
