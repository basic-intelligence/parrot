import languageCatalog from "../native-core/shared/languages.json";

export type DictationLanguageMode = "english" | "detect" | "specific";
export type CleanupModelId = string;

export type LanguageOption = {
  code: string;
  speechCode: string;
  name: string;
  nativeName: string;
  variantOf?: string;
};

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
