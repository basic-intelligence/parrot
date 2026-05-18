use parrot_protocol::{AppSettings, DictationLanguageMode};
use serde::{Deserialize, Serialize};

pub const LANGUAGE_CATALOG_JSON: &str = include_str!("../../../native-core/shared/languages.json");

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LanguageOption {
    pub code: String,
    pub speech_code: String,
    pub name: String,
    pub native_name: String,
    #[serde(default)]
    pub variant_of: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DictationLanguageSettings {
    pub dictation_language_mode: DictationLanguageMode,
    #[serde(default)]
    pub dictation_language_code: Option<String>,
    pub cleanup_model_id: String,
}

impl From<&AppSettings> for DictationLanguageSettings {
    fn from(settings: &AppSettings) -> Self {
        Self {
            dictation_language_mode: settings.dictation_language_mode.clone(),
            dictation_language_code: settings.dictation_language_code.clone(),
            cleanup_model_id: settings.cleanup_model_id.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum SpeechModelSlot {
    Speech,
    SpeechMultilingual,
}

impl SpeechModelSlot {
    pub fn public_id(self) -> &'static str {
        match self {
            Self::Speech => "speech",
            Self::SpeechMultilingual => "speech-multilingual",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DictationLanguageMetadata {
    pub mode: String,
    #[serde(default)]
    pub code: Option<String>,
    #[serde(default)]
    pub locale: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
}

impl DictationLanguageMetadata {
    pub fn xml_element(&self) -> String {
        let mut attributes = vec![format!("mode=\"{}\"", escape_xml(&self.mode))];
        if let Some(code) = self.code.as_deref().filter(|value| !value.is_empty()) {
            attributes.push(format!("code=\"{}\"", escape_xml(code)));
        }
        if let Some(locale) = self.locale.as_deref().filter(|value| !value.is_empty()) {
            attributes.push(format!("locale=\"{}\"", escape_xml(locale)));
        }
        if let Some(name) = self.name.as_deref().filter(|value| !value.is_empty()) {
            attributes.push(format!("name=\"{}\"", escape_xml(name)));
        }
        format!("<dictation_language {} />", attributes.join(" "))
    }
}

pub fn language_catalog() -> Vec<LanguageOption> {
    serde_json::from_str(LANGUAGE_CATALOG_JSON).expect("shared language catalog must be valid JSON")
}

pub fn canonical_language_code(code: &str) -> Option<String> {
    let normalized = normalize_language_code(code)?;
    language_catalog()
        .into_iter()
        .find(|language| language.code.eq_ignore_ascii_case(&normalized))
        .map(|language| language.code)
        .or(Some(normalized))
}

pub fn speech_code_for_language_code(code: &str) -> Option<String> {
    let canonical = canonical_language_code(code)?;
    language_catalog()
        .into_iter()
        .find(|language| language.code.eq_ignore_ascii_case(&canonical))
        .and_then(|language| normalize_language_code(&language.speech_code))
        .or_else(|| normalize_language_code(&canonical))
}

pub fn uses_english_route(settings: &DictationLanguageSettings) -> bool {
    match settings.dictation_language_mode {
        DictationLanguageMode::English => true,
        DictationLanguageMode::Detect => false,
        DictationLanguageMode::Specific => {
            settings
                .dictation_language_code
                .as_deref()
                .and_then(speech_code_for_language_code)
                .as_deref()
                == Some("en")
        }
    }
}

pub fn speech_model_slot(settings: &DictationLanguageSettings) -> SpeechModelSlot {
    if uses_english_route(settings) {
        SpeechModelSlot::Speech
    } else {
        SpeechModelSlot::SpeechMultilingual
    }
}

pub fn decode_language_code(settings: &DictationLanguageSettings) -> Option<String> {
    match settings.dictation_language_mode {
        DictationLanguageMode::English => Some("en".to_string()),
        DictationLanguageMode::Specific => settings
            .dictation_language_code
            .as_deref()
            .and_then(speech_code_for_language_code),
        DictationLanguageMode::Detect => None,
    }
}

pub fn selected_language_metadata(
    settings: &DictationLanguageSettings,
) -> DictationLanguageMetadata {
    match settings.dictation_language_mode {
        DictationLanguageMode::English => DictationLanguageMetadata {
            mode: "selected".into(),
            code: Some("en".into()),
            locale: None,
            name: Some("English".into()),
        },
        DictationLanguageMode::Specific => {
            let selected_code = settings
                .dictation_language_code
                .as_deref()
                .and_then(canonical_language_code);
            let speech_code = selected_code
                .as_deref()
                .and_then(speech_code_for_language_code);
            let language = selected_code
                .as_deref()
                .and_then(|code| language_by_code(code));
            let locale = match (selected_code.as_deref(), speech_code.as_deref()) {
                (Some(selected), Some(speech)) if !selected.eq_ignore_ascii_case(speech) => {
                    Some(selected.to_string())
                }
                _ => None,
            };

            DictationLanguageMetadata {
                mode: "selected".into(),
                code: speech_code,
                locale,
                name: language.map(|language| language.name),
            }
        }
        DictationLanguageMode::Detect => DictationLanguageMetadata {
            mode: "detected".into(),
            code: None,
            locale: None,
            name: Some("unknown".into()),
        },
    }
}

pub fn detected_language_metadata(code: Option<&str>) -> DictationLanguageMetadata {
    let Some(normalized) = code.and_then(normalize_language_code) else {
        return DictationLanguageMetadata {
            mode: "detected".into(),
            code: None,
            locale: None,
            name: Some("unknown".into()),
        };
    };

    DictationLanguageMetadata {
        mode: "detected".into(),
        code: Some(normalized),
        locale: None,
        name: None,
    }
}

pub fn language_by_code(code: &str) -> Option<LanguageOption> {
    let normalized = normalize_language_code(code)?;
    language_catalog()
        .into_iter()
        .find(|language| language.code.eq_ignore_ascii_case(&normalized))
}

fn normalize_language_code(code: &str) -> Option<String> {
    let normalized = code.trim().to_ascii_lowercase();
    (!normalized.is_empty()).then_some(normalized)
}

fn escape_xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct LanguageRoutingFixture {
        settings: DictationLanguageSettings,
        uses_english_route: bool,
        speech_model_slot: SpeechModelSlot,
        decode_language_code: Option<String>,
        selected_language_xml: String,
        cleanup_model_id: String,
    }

    #[test]
    fn routes_match_shared_fixtures() {
        let fixtures: Vec<LanguageRoutingFixture> = serde_json::from_str(include_str!(
            "../../../native-core/shared/test-fixtures/language-routing.json"
        ))
        .unwrap();

        for fixture in fixtures {
            assert_eq!(
                uses_english_route(&fixture.settings),
                fixture.uses_english_route
            );
            assert_eq!(
                speech_model_slot(&fixture.settings),
                fixture.speech_model_slot
            );
            assert_eq!(
                decode_language_code(&fixture.settings),
                fixture.decode_language_code
            );
            assert_eq!(
                selected_language_metadata(&fixture.settings).xml_element(),
                fixture.selected_language_xml
            );
            assert_eq!(fixture.settings.cleanup_model_id, fixture.cleanup_model_id);
        }
    }

    #[test]
    fn canonicalizes_catalog_locale_casing() {
        assert_eq!(canonical_language_code(" pt-br ").as_deref(), Some("pt-BR"));
        assert_eq!(
            speech_code_for_language_code("pt-BR").as_deref(),
            Some("pt")
        );
    }
}
