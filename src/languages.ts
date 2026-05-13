import languageCatalog from "../native-core/shared/languages.json";

export type DictationLanguageMode = "english" | "detect" | "specific";

export type DictationLanguageSettings = {
  dictationLanguageMode: DictationLanguageMode;
  dictationLanguageCode: string | null;
  cleanupModelId: string;
};

export type ModelId = string;
export type CleanupModelId = string;

export type LanguageOption = {
  code: string;
  speechCode: string;
  name: string;
  nativeName: string;
  variantOf?: string;
};

export const MODEL_IDS = {
  englishSpeech: "speech",
  multilingualSpeech: "speech-multilingual",
} as const satisfies Record<string, ModelId>;

export const LANGUAGE_OPTIONS: LanguageOption[] = (
  languageCatalog as LanguageOption[]
).map((language) => ({ ...language }));

export const SPECIFIC_LANGUAGE_OPTIONS = LANGUAGE_OPTIONS.filter(
  (language) => language.code !== "en",
);

function normalizedLanguageCode(code: string | null | undefined) {
  return code?.trim().toLowerCase() ?? "";
}

export function languageByCode(code: string | null | undefined) {
  const normalized = normalizedLanguageCode(code);
  if (!normalized) return null;

  return (
    LANGUAGE_OPTIONS.find(
      (language) => language.code.toLowerCase() === normalized,
    ) || null
  );
}

export function languageDisplayValue(language: LanguageOption) {
  return language.name === language.nativeName
    ? language.name
    : `${language.name} (${language.nativeName})`;
}

export function speechCodeForLanguageCode(code: string | null | undefined) {
  const language = languageByCode(code);
  return language?.speechCode ?? null;
}

export function isEnglishSpeechRoute(code: string | null | undefined) {
  return speechCodeForLanguageCode(code) === "en";
}

export function usesEnglishRoute(settings: DictationLanguageSettings) {
  return (
    settings.dictationLanguageMode === "english" ||
    (settings.dictationLanguageMode === "specific" &&
      isEnglishSpeechRoute(settings.dictationLanguageCode))
  );
}

export function requiredModelIds(settings: DictationLanguageSettings): ModelId[] {
  if (usesEnglishRoute(settings)) {
    return [MODEL_IDS.englishSpeech, selectedCleanupModelId(settings)];
  }
  return [MODEL_IDS.multilingualSpeech, selectedCleanupModelId(settings)];
}

export function selectedCleanupModelId(
  settings: DictationLanguageSettings,
): CleanupModelId {
  return settings.cleanupModelId;
}
