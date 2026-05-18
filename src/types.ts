import type { DictationLanguageMode } from "./languages";

export type AudioDevice = { uid: string; name: string; isDefault: boolean };

export type ModelStatus = {
  id: string;
  role: "speech" | "cleanup";
  displayName: string;
  subtitle: string;
  expectedBytes: number;
  localBytes: number;
  progressBytes: number;
  progressTotalBytes: number;
  downloaded: boolean;
  downloading: boolean;
  required: boolean;
  error: string | null;
};

export type HistoryEntry = {
  id: string;
  createdAt: string;
  audioDurationSeconds: number;
  rawTranscription: string | null;
  cleanedTranscription: string | null;
};

export type DictionaryEntry = {
  id: string;
  term: string;
};

export type ShortcutModifier =
  | "command"
  | "control"
  | "option"
  | "alt"
  | "shift"
  | "fn"
  | "meta";

export type ShortcutKey =
  | "space"
  | "return"
  | "tab"
  | "escape"
  | "arrowLeft"
  | "arrowRight"
  | "arrowUp"
  | "arrowDown"
  | "delete"
  | { character: string }
  | { function: number };

export type ShortcutChord = {
  modifiers: ShortcutModifier[];
  key: ShortcutKey | null;
};

export type ShortcutPlatformCodes = {
  macosKeyCodes: number[] | null;
  windowsVirtualKeys: number[] | null;
  linuxKeyCodes: number[] | null;
};

export type ShortcutSettings = {
  displayName: string;
  mode: "hold" | "toggle";
  enabled: boolean;
  doubleTapToggle: boolean;
  chord: ShortcutChord | null;
  platformCodes: ShortcutPlatformCodes;
  macosKeyCodes?: number[];
};

export type AppSettings = {
  selectedInputUid: string | null;
  pushToTalkShortcut: ShortcutSettings;
  handsFreeShortcut: ShortcutSettings;
  dictationLanguageMode: DictationLanguageMode;
  dictationLanguageCode: string | null;
  cleanupModelId: string;
  cleanupEnabled: boolean;
  cleanupPrompt: string;
  dictionaryEntries: DictionaryEntry[];
  playSounds: boolean;
  pasteIntoRecordingStartWindow: boolean;
  historyEnabled: boolean;
  launchAtLogin: boolean;
  onboardingCompleted: boolean;
  inputMonitoringPermissionShownInOnboarding: boolean;
};

export type PermissionState =
  | "granted"
  | "denied"
  | "notDetermined"
  | "unknown"
  | string;

export type PermissionKind =
  | "microphone"
  | "accessibility"
  | "inputMonitoring"
  | "globalShortcut"
  | "paste"
  | "focusedTextContext";

export type PermissionRequirement = {
  kind: PermissionKind;
  title: string;
  description: string;
  state: PermissionState;
  required: boolean;
  requestable: boolean;
  opensSettings: boolean;
};

export type PermissionSnapshot = {
  requirements?: PermissionRequirement[];
  allRequiredGranted?: boolean;
  microphone?: PermissionState | null;
  accessibility?: PermissionState | null;
  inputMonitoring?: PermissionState | null;
  allGranted?: boolean | null;
};

export type Snapshot = {
  settings: AppSettings;
  devices: AudioDevice[];
  models: ModelStatus[];
  history: HistoryEntry[];
  permissions: PermissionSnapshot;
  defaultCleanupPrompt: string;
};

export type RecordingEvent = {
  raw: string;
  cleaned: string;
  audioDurationSeconds: number;
  kind?: string;
};

export type DictationResult = {
  raw: string;
  cleaned: string;
};

export type RecordingResultPayload = {
  raw: string;
  cleaned: string;
  audioDurationSeconds: number;
};

