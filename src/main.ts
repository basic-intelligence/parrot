import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow, LogicalSize } from "@tauri-apps/api/window";
import { openUrl } from "@tauri-apps/plugin-opener";
import { relaunch } from "@tauri-apps/plugin-process";
import {
  check,
  type DownloadEvent,
  type Update,
} from "@tauri-apps/plugin-updater";
import packageJson from "../package.json";
import {
  MODEL_IDS,
  SPECIFIC_LANGUAGE_OPTIONS,
  selectedCleanupModelId,
  type CleanupModelId,
  type DictationLanguageMode,
  languageByCode,
  languageDisplayValue,
  requiredModelIds,
  usesEnglishRoute,
} from "./languages";
import "./style.css";

type AudioDevice = { uid: string; name: string; isDefault: boolean };
type ModelStatus = {
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
type HistoryEntry = {
  id: string;
  createdAt: string;
  audioDurationSeconds: number;
  rawTranscription: string | null;
  cleanedTranscription: string | null;
};
type DictionaryEntry = {
  id: string;
  term: string;
};
type AppSettings = {
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
  historyEnabled: boolean;
  launchAtLogin: boolean;
  onboardingCompleted: boolean;
  inputMonitoringPermissionShownInOnboarding: boolean;
};
type ShortcutSettings = {
  displayName: string;
  macosKeyCodes: number[];
  mode: "hold" | "toggle";
  enabled: boolean;
  doubleTapToggle: boolean;
};
type ShortcutSettingKey = "pushToTalkShortcut" | "handsFreeShortcut";
type ShortcutNotice = {
  level: "info" | "success" | "error";
  message: string;
};
type CleanupModelSelectionNotice = {
  id: CleanupModelId;
  message: string;
};
type Snapshot = {
  settings: AppSettings;
  devices: AudioDevice[];
  models: ModelStatus[];
  history: HistoryEntry[];
  permissions: PermissionSnapshot;
  defaultCleanupPrompt: string;
};
type PermissionState =
  | "granted"
  | "denied"
  | "notDetermined"
  | "unknown"
  | string;
type PermissionKind = "microphone" | "accessibility" | "inputMonitoring";
type PermissionSnapshot = {
  microphone: PermissionState;
  accessibility: PermissionState;
  inputMonitoring: PermissionState;
  allGranted: boolean;
};
type PermissionRowOptions = {
  hideRefreshWhenGranted?: boolean;
  openSettingsWhenGranted?: boolean;
  variant?: "setup" | "settings";
};
type UpdateCheckSource = "manual" | "automatic";
type UpdateStatus = "idle" | "current" | "available" | "error";
type UpdateDownloadProgress = {
  downloadedBytes: number;
  totalBytes: number | null;
};
type RecordingEvent = {
  raw: string;
  cleaned: string;
  audioDurationSeconds: number;
  kind?: string;
};
type MainTab = "general" | "recording" | "cleanup" | "history" | "about";
type SetupStep = "permissions" | "language" | "models";
type IconName =
  | "general"
  | "recording"
  | "cleanup"
  | "history"
  | "about"
  | "download"
  | "delete"
  | "copy"
  | "check"
  | "mic"
  | "clipboard"
  | "keyboard"
  | "externalLink"
  | "fileText"
  | "shield"
  | "star"
  | "bug"
  | "mail";

const ICONS: Record<IconName, string> = {
  general:
    '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M4 21v-7"/><path d="M4 10V3"/><path d="M12 21v-9"/><path d="M12 8V3"/><path d="M20 21v-5"/><path d="M20 12V3"/><path d="M2 14h4"/><path d="M10 8h4"/><path d="M18 16h4"/></svg>',
  recording:
    '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 3a3 3 0 0 0-3 3v6a3 3 0 0 0 6 0V6a3 3 0 0 0-3-3z"/><path d="M5 11a7 7 0 0 0 14 0"/><path d="M12 18v3"/><path d="M8.5 21h7"/></svg>',
  cleanup:
    '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M3 21l12-12"/><path d="M14 8l2 2"/><path d="M7 14l3 3"/><path d="M15 4V2"/><path d="M15 8V6"/><path d="M12 5h-2"/><path d="M20 5h-2"/><path d="M18 3l-1 1"/><path d="M18 7l-1-1"/></svg>',
  history:
    '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M3 12a9 9 0 1 0 3-6.7"/><path d="M3 3v6h6"/><path d="M12 7v5l3 2"/></svg>',
  about:
    '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18z"/><path d="M12 11v5"/><path d="M12 8h.01"/></svg>',
  download:
    '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 4v10"/><path d="M8 10l4 4 4-4"/><path d="M5 19h14"/></svg>',
  delete:
    '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M7 7h10"/><path d="M10 7V5h4v2"/><path d="M9 10v7M15 10v7"/><path d="M8 7l1 13h6l1-13"/></svg>',
  copy:
    '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M8 8h10v12H8z"/><path d="M6 16H4V4h10v2"/><path d="M11 12h4M11 16h4"/></svg>',
  check:
    '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M5 12.5l4.5 4.5L19 7"/></svg>',
  mic:
    '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 3a3 3 0 0 0-3 3v6a3 3 0 0 0 6 0V6a3 3 0 0 0-3-3z"/><path d="M5 11a7 7 0 0 0 14 0"/><path d="M12 18v3"/><path d="M8.5 21h7"/></svg>',
  clipboard:
    '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M9 5h6"/><path d="M10 3h4l1 2h3v16H6V5h3l1-2z"/><path d="M9 13h5"/><path d="M13 10l3 3-3 3"/></svg>',
  keyboard:
    '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M4 7h16v10H4z"/><path d="M7 10h.01M10 10h.01M13 10h.01M16 10h.01M8 14h8"/></svg>',
  externalLink:
    '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M15 3h6v6"/><path d="M10 14L21 3"/><path d="M21 14v5a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5"/></svg>',
  fileText:
    '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M14 3H6a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V9z"/><path d="M14 3v6h6"/><path d="M8 13h8"/><path d="M8 17h6"/></svg>',
  shield:
    '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 3l7 3v5c0 5-3.5 8.5-7 10-3.5-1.5-7-5-7-10V6l7-3z"/><path d="M9.5 12l1.8 1.8L15.5 9.8"/></svg>',
  star:
    '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 3l2.7 5.5 6.1.9-4.4 4.3 1 6.1L12 16.9 6.6 19.8l1-6.1-4.4-4.3 6.1-.9L12 3z"/></svg>',
  bug:
    '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M8 8a4 4 0 0 1 8 0v1H8z"/><path d="M8 9h8v6a4 4 0 0 1-8 0z"/><path d="M4 13h4"/><path d="M16 13h4"/><path d="M5 19l3-2"/><path d="M19 19l-3-2"/><path d="M5 7l3 2"/><path d="M19 7l-3 2"/><path d="M10 4L8 2"/><path d="M14 4l2-2"/></svg>',
  mail:
    '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M4 6h16v12H4z"/><path d="M4 7l8 6 8-6"/></svg>',
};

const SETUP_STEPS: Array<{
  id: SetupStep;
  label: string;
  heading: string;
  description: string;
}> = [
  {
    id: "permissions",
    label: "Permissions",
    heading: "Enable Parrot",
    description:
      "Choose which language(s) you'll speak, enable the required macOS permissions, then download their local models Parrot needs.",
  },
  {
    id: "language",
    label: "Languages",
    heading: "Choose language",
    description:
      "Tell Parrot which language or locale to prepare. English locales use the fastest path; Detect and other languages use multilingual models.",
  },
  {
    id: "models",
    label: "Models",
    heading: "Download models",
    description:
      "Download the local speech and cleanup models required for your selected language path.",
  },
];

const app = document.querySelector("#app")!;
const MAIN_WINDOW_SIZE = new LogicalSize(920, 680);
const SETUP_WINDOW_SIZE = new LogicalSize(600, 904);
let snapshot: Snapshot | null = null;
let activeTab: MainTab = "general";
let setupStep: SetupStep = "permissions";
let setupFinalizing = false;
let setupPollHandle: number | null = null;
let setupScrollTop = 0;
let inputMonitoringFallbackRequired = false;
let activeWindowLayout: "setup" | "main" | null = null;
let lastRenderWasSetupGate = false;
let testRecording = false;
let testResult = "";
let toast = "";
let eventListenersInstalled = false;
let modelPollHandle: number | null = null;
let confirmClearHistoryOpen = false;
let clearHistoryBusy = false;
const historyCopyFeedbackTimers = new WeakMap<HTMLButtonElement, number>();
let cleanupModelSelectionNotice: CleanupModelSelectionNotice | null = null;
let shortcutCaptureTarget: ShortcutSettingKey | null = null;
let shortcutNotice: ShortcutNotice | null = null;
let shortcutCaptureSession = 0;
let shortcutMonitorPausedForCapture = false;
let updateStatus: UpdateStatus = "idle";
let updateInfo: Update | null = null;
let checkingForUpdate = false;
let installingUpdate = false;
let updateDownloadProgress: UpdateDownloadProgress | null = null;
let updateDownloadFinished = false;
let lastUpdateProgressRenderAt = 0;
const modelFinalizingStartedAt = new Map<string, number>();

const DEFAULT_PUSH_TO_TALK_SHORTCUT: ShortcutSettings = {
  displayName: "Fn",
  macosKeyCodes: [63],
  mode: "hold",
  enabled: true,
  doubleTapToggle: false,
};

const DEFAULT_HANDS_FREE_SHORTCUT: ShortcutSettings = {
  displayName: "Control + Space",
  macosKeyCodes: [59, 49],
  mode: "toggle",
  enabled: true,
  doubleTapToggle: false,
};

const APP_VERSION = packageJson.version;
const PARROT_REPO_URL = "https://github.com/basic-intelligence/parrot";
const BUG_REPORT_URL = `${PARROT_REPO_URL}/issues/new?template=bug_report.yml`;
const FEATURE_REQUEST_URL = `${PARROT_REPO_URL}/issues/new?template=feature_request.yml`;
const CONTRIBUTING_URL = `${PARROT_REPO_URL}/blob/main/CONTRIBUTING.md`;
const LICENSE_URL = `${PARROT_REPO_URL}/blob/main/LICENSE`;
const PRIVACY_URL = `${PARROT_REPO_URL}/blob/main/PRIVACY.md`;
const THIRD_PARTY_LICENSES_URL = `${PARROT_REPO_URL}/blob/main/THIRD_PARTY_LICENSES.md`;
const EMAIL_URL = "mailto:richard@basic.in?subject=Parrot%20feedback";
const AUTO_UPDATE_CHECK_DELAY_MS = 4_000;
const AUTO_UPDATE_CHECK_INTERVAL_MS = 12 * 60 * 60 * 1000;
const LAST_UPDATE_CHECK_KEY = "parrot:lastUpdateCheckAt";

async function boot() {
  await installEventListeners();
  await load();
  scheduleAutomaticUpdateChecks();
}

async function installEventListeners() {
  if (eventListenersInstalled) return;
  eventListenersInstalled = true;

  await listen("parrot:recording-started", () => {
    // The floating overlay owns normal recording state.
  });
  await listen("parrot:recording-processing", () => {
    // The floating overlay owns normal transcription state.
  });
  await listen<RecordingEvent>("parrot:recording-finished", async (event) => {
    try {
      if (event.payload.kind !== "test") {
        snapshot = await invoke<Snapshot>("save_recording_result", {
          result: event.payload,
        });
        if (activeTab === "history") render();
      } else {
        await load();
      }
    } catch (error) {
      toast = `Dictation pasted, but history could not be saved: ${errorMessage(error)}`;
      render();
    }
  });
  await listen("parrot:recording-cancelled", () => {
    // Intentional user cancellation: do not save history and do not show an error toast.
    toast = "";
    render();
  });
  await listen<{ error?: string }>("parrot:recording-failed", (event) => {
    toast = event.payload.error || "Recording failed.";
    render();
  });
  await listen<{ error?: string }>("parrot:hotkey-monitor-failed", (event) => {
    const message =
      event.payload.error ||
      "Shortcut monitor failed. Check Accessibility permission.";
    void maybeRequireInputMonitoring(message);
    toast = message;
    render();
  });
  await listen<{ error?: string }>("parrot:native-core-disconnected", (event) => {
    testRecording = false;
    if (testResult === "Transcribing…" || testResult === "Transcribing...") {
      testResult = "Parrot Core crashed while transcribing. This recording was lost.";
    }
    toast =
      event.payload.error ||
      "Parrot Core disconnected. Parrot will try to restart it automatically.";
    render();
  });
  await listen("parrot:native-core-recovered", () => {
    toast = "Parrot Core restarted.";
    render();
  });
  await listen<{ tab?: MainTab }>("parrot:open-settings", (event) => {
    if (event.payload.tab === "general") {
      activeTab = "general";
      toast = "";
      confirmClearHistoryOpen = false;
      render();
    }
  });
}

async function load() {
  let next = await invoke<Snapshot>("get_app_snapshot");
  const firstLoad = snapshot === null;

  if (
    firstLoad &&
    setupRequirementsComplete(next) &&
    !next.settings.onboardingCompleted
  ) {
    next = await invoke<Snapshot>("save_settings", {
      settings: { ...next.settings, onboardingCompleted: true },
    });
  }

  snapshot = next;
  render();
}

async function saveSettings(partial: Partial<AppSettings>) {
  if (!snapshot) return;
  const next = { ...snapshot.settings, ...partial };
  snapshot = await invoke<Snapshot>("save_settings", { settings: next });
  render();
}

async function checkForUpdatesManually() {
  await checkForUpdates("manual");
}

async function checkForUpdates(source: UpdateCheckSource) {
  if (checkingForUpdate || installingUpdate) return;

  const manual = source === "manual";
  checkingForUpdate = true;

  if (manual) {
    updateStatus = "idle";
    updateInfo = null;
    updateDownloadProgress = null;
    updateDownloadFinished = false;
    toast = "";
    render();
  }

  try {
    const update = await check({ timeout: 30_000 });

    localStorage.setItem(LAST_UPDATE_CHECK_KEY, String(Date.now()));

    if (update) {
      updateStatus = "available";
      updateInfo = update;
      await setUpdateBadge(true, update.version);
    } else {
      updateStatus = "current";
      updateInfo = null;
      await setUpdateBadge(false, null);
    }
  } catch (error) {
    if (manual) {
      updateStatus = "error";
      toast = "";
    } else {
      console.debug("Automatic update check failed", error);
    }
  } finally {
    checkingForUpdate = false;
    render();
  }
}

function scheduleAutomaticUpdateChecks() {
  window.setTimeout(() => {
    void maybeRunAutomaticUpdateCheck();
  }, AUTO_UPDATE_CHECK_DELAY_MS);

  window.setInterval(() => {
    void maybeRunAutomaticUpdateCheck();
  }, AUTO_UPDATE_CHECK_INTERVAL_MS);
}

async function maybeRunAutomaticUpdateCheck() {
  const lastCheckedAt = Number(localStorage.getItem(LAST_UPDATE_CHECK_KEY) || 0);
  const stale = Date.now() - lastCheckedAt > AUTO_UPDATE_CHECK_INTERVAL_MS;

  if (stale) {
    await checkForUpdates("automatic");
  }
}

async function setUpdateBadge(available: boolean, version: string | null) {
  try {
    await invoke("set_update_badge", { available, version });
  } catch (error) {
    console.debug("Could not update tray badge", error);
  }
}

async function installParrotUpdate() {
  if (!updateInfo || checkingForUpdate || installingUpdate) return;

  const pendingUpdate = updateInfo;
  installingUpdate = true;
  updateDownloadProgress = { downloadedBytes: 0, totalBytes: null };
  updateDownloadFinished = false;
  lastUpdateProgressRenderAt = 0;
  toast = "";
  render();

  try {
    await pendingUpdate.downloadAndInstall(acceptUpdateDownloadEvent);
    await relaunch();
  } catch (error) {
    installingUpdate = false;
    updateDownloadProgress = null;
    updateDownloadFinished = false;
    toast = `Could not install update. ${friendlyUpdateError(error)}`;
    render();
  }
}

function acceptUpdateDownloadEvent(event: DownloadEvent) {
  if (event.event === "Started") {
    updateDownloadProgress = {
      downloadedBytes: 0,
      totalBytes: event.data.contentLength ?? null,
    };
    lastUpdateProgressRenderAt = Date.now();
    render();
    return;
  }

  if (event.event === "Progress") {
    const previous = updateDownloadProgress ?? {
      downloadedBytes: 0,
      totalBytes: null,
    };
    updateDownloadProgress = {
      ...previous,
      downloadedBytes: previous.downloadedBytes + event.data.chunkLength,
    };

    const now = Date.now();
    if (now - lastUpdateProgressRenderAt > 150) {
      lastUpdateProgressRenderAt = now;
      render();
    }
    return;
  }

  const downloadedBytes = updateDownloadProgress?.downloadedBytes ?? 0;
  updateDownloadProgress = {
    downloadedBytes,
    totalBytes: updateDownloadProgress?.totalBytes ?? (downloadedBytes || null),
  };
  updateDownloadFinished = true;
  render();
}

function requiredModelsDownloaded(models: ModelStatus[], settings: AppSettings) {
  const requiredByBackend = models.filter((model) => model.required);
  if (requiredByBackend.length > 0) {
    return requiredByBackend.every((model) => model.downloaded);
  }

  const required = requiredModelIds(settings);
  return required.every((id) =>
    Boolean(models.find((model) => model.id === id)?.downloaded),
  );
}

function setupRequirementsComplete(value: Snapshot) {
  return (
    setupPermissionsComplete(value.permissions, value.settings) &&
    requiredModelsDownloaded(value.models, value.settings)
  );
}

function inputMonitoringShownInOnboarding(
  settings: AppSettings | null | undefined,
) {
  return (
    inputMonitoringFallbackRequired ||
    settings?.inputMonitoringPermissionShownInOnboarding === true
  );
}

function setupPermissionsComplete(
  permissions: PermissionSnapshot,
  settings: AppSettings | null | undefined,
) {
  return (
    permissions.microphone === "granted" &&
    permissions.accessibility === "granted" &&
    (!inputMonitoringShownInOnboarding(settings) ||
      permissions.inputMonitoring === "granted")
  );
}

function shouldShowInputMonitoringPermission(settings: AppSettings) {
  return settings.inputMonitoringPermissionShownInOnboarding;
}

async function persistInputMonitoringShownInOnboarding() {
  if (
    !snapshot ||
    snapshot.settings.onboardingCompleted ||
    snapshot.settings.inputMonitoringPermissionShownInOnboarding
  ) {
    return;
  }

  const settings = {
    ...snapshot.settings,
    inputMonitoringPermissionShownInOnboarding: true,
  };
  snapshot = { ...snapshot, settings };

  try {
    snapshot = await invoke<Snapshot>("save_settings", { settings });
  } catch (error) {
    console.warn("Could not save Input Monitoring onboarding state", error);
  }
}

async function maybeRequireInputMonitoring(message: string) {
  if (
    /input monitoring|keyboard event tap|listen for global shortcuts?|listen for the global shortcut/i.test(
      message,
    )
  ) {
    if (!snapshot || !snapshot.settings.onboardingCompleted) {
      inputMonitoringFallbackRequired = true;
      setupStep = "permissions";
      await persistInputMonitoringShownInOnboarding();
    }
  }
}

function deviceOptions(devices: AudioDevice[], selected: string | null) {
  const defaultDevice = devices.find((d) => d.isDefault);
  const options = [
    `<option value="">System Default${defaultDevice ? ` — ${escapeHtml(defaultDevice.name)}` : ""}</option>`,
  ];
  for (const device of devices) {
    const selectedAttr = device.uid === selected ? "selected" : "";
    options.push(
      `<option value="${escapeAttr(device.uid)}" ${selectedAttr}>${escapeHtml(device.name)}${device.isDefault ? " (default)" : ""}</option>`,
    );
  }
  return options.join("");
}

function render() {
  if (!snapshot) {
    app.innerHTML = '<div class="loading">Loading…</div>';
    return;
  }

  syncModelFinalizingTimers(snapshot.models);

  const shouldShowSetupGate = !snapshot.settings.onboardingCompleted;
  const enteringSetupGate = !lastRenderWasSetupGate && shouldShowSetupGate;
  const leavingSetupGate = lastRenderWasSetupGate && !shouldShowSetupGate;
  lastRenderWasSetupGate = shouldShowSetupGate;

  if (enteringSetupGate) {
    void applyMainWindowLayout("setup");
  }

  if (leavingSetupGate) {
    activeTab = "general";
    void applyMainWindowLayout("main");
  }

  if (shouldShowSetupGate) {
    const setupPanel = document.querySelector<HTMLElement>(".setup-panel");
    setupScrollTop = setupPanel?.scrollTop ?? setupScrollTop;
    app.innerHTML = renderSetupGate(snapshot);
    bindSetupEvents();
    syncSetupPolling();
    restoreSetupScrollPosition();
    return;
  }

  syncSetupPolling();

  const { settings, devices, history, models } = snapshot;
  const navDisabled = shortcutCaptureTarget ? "disabled" : "";
  const tabs: Array<[MainTab, string, IconName]> = [
    ["general", "General", "general"],
    ["recording", "Recording", "recording"],
    ["cleanup", "Cleanup", "cleanup"],
    ["history", "History", "history"],
    ["about", "About", "about"],
  ];

  app.innerHTML = `
    <section class="shell">
      <aside class="sidebar">
        <div class="brand">
          <div class="brand-logo-wrap">
            <img class="brand-logo" src="/logo_no_background.png?v=2" alt="" />
            ${
              updateStatus === "available"
                ? '<span class="brand-update-dot" aria-label="Update available"></span>'
                : ""
            }
          </div>
          <div>
            <h1>Parrot</h1>
          </div>
        </div>
        <nav>
          ${tabs
            .map(
              ([id, label, iconName]) => `
            <button class="nav ${activeTab === id ? "active" : ""}" data-tab="${id}" ${navDisabled}>
              <span class="nav-icon">${icon(iconName)}</span>
              <span>${label}</span>
              ${
                id === "general" && updateStatus === "available"
                  ? '<span class="nav-update-dot" aria-hidden="true"></span>'
                  : ""
              }
            </button>
          `,
            )
            .join("")}
        </nav>
        <div class="sidebar-spacer" aria-hidden="true"></div>
        <div class="sidebar-community" aria-label="Community links">
          <button
            class="sidebar-link external-link"
            type="button"
            data-external-url="${escapeAttr(BUG_REPORT_URL)}"
            ${navDisabled}
          >
            <span class="sidebar-link-icon">${icon("bug")}</span>
            <span>Report Bug</span>
          </button>
          <button
            class="sidebar-link external-link"
            type="button"
            data-external-url="${escapeAttr(PARROT_REPO_URL)}"
            ${navDisabled}
          >
            <span class="sidebar-link-icon">${icon("star")}</span>
            <span>Star on GitHub</span>
          </button>
        </div>
      </aside>
      <section class="content">
        ${toast ? renderToast(toast) : ""}
        ${activeTab === "general" ? renderGeneral(settings, snapshot.permissions, models) : ""}
        ${activeTab === "recording" ? renderRecording(settings, devices) : ""}
        ${activeTab === "cleanup" ? renderCleanup(
          settings,
          models,
          snapshot.defaultCleanupPrompt,
        ) : ""}
        ${activeTab === "history" ? renderHistory(settings, history) : ""}
        ${activeTab === "about" ? renderAbout() : ""}
      </section>
    </section>
    ${confirmClearHistoryOpen ? renderClearHistoryConfirm() : ""}
  `;

  bindEvents();

  if (leavingSetupGate) {
    scrollPageToTop();
  }
}

function selectMainTab(tab: MainTab) {
  activeTab = tab;
  toast = "";
  confirmClearHistoryOpen = false;

  render();
}

function renderClearHistoryConfirm() {
  const disabled = clearHistoryBusy ? "disabled" : "";

  return `
    <div class="modal-backdrop" role="presentation">
      <section
        class="modal-panel"
        role="alertdialog"
        aria-modal="true"
        aria-labelledby="clearHistoryTitle"
        aria-describedby="clearHistoryDescription"
      >
        <h3 id="clearHistoryTitle">Clear all transcripts?</h3>
        <p id="clearHistoryDescription">Are you sure you want to delete all saved transcripts? This cannot be undone.</p>
        <div class="modal-actions">
          <button id="cancelClearHistory" class="secondary" ${disabled}>Cancel</button>
          <button id="confirmClearHistory" class="danger" ${disabled}>${clearHistoryBusy ? "Clearing…" : "Yes, clear transcripts"}</button>
        </div>
      </section>
    </div>
  `;
}

function setupStepIndex(step: SetupStep) {
  return SETUP_STEPS.findIndex((item) => item.id === step);
}

function setupStepMeta(step: SetupStep) {
  return SETUP_STEPS[setupStepIndex(step)] || SETUP_STEPS[0];
}

function syncSetupStepWithSnapshot(current: Snapshot) {
  if (!setupPermissionsComplete(current.permissions, current.settings)) {
    setupStep = "permissions";
  }
}

function setupStepComplete(current: Snapshot, step: SetupStep) {
  if (step === "permissions")
    return setupPermissionsComplete(current.permissions, current.settings);
  if (step === "language") return true;
  return requiredModelsDownloaded(current.models, current.settings);
}

function renderSetupProgress() {
  const currentIndex = setupStepIndex(setupStep);

  return `
    <div class="setup-progress" aria-label="Onboarding progress">
      ${SETUP_STEPS.map((step, index) => {
        const stateClass =
          index < currentIndex
            ? "complete"
            : index === currentIndex
              ? "active"
              : "";

        return `
          <div class="setup-progress-item ${stateClass}">
            <span class="setup-progress-bar" aria-hidden="true"></span>
            <span class="setup-progress-label">${escapeHtml(step.label)}</span>
          </div>
        `;
      }).join("")}
    </div>
  `;
}

function renderSetupStepContent(current: Snapshot) {
  const permissions = current.permissions;
  const showInputMonitoring = inputMonitoringShownInOnboarding(
    current.settings,
  );

  if (setupStep === "permissions") {
    return `
      <section class="setup-section">
        <div class="section-heading">
          <h3>Permissions</h3>
          <p>${showInputMonitoring ? "Microphone records dictation. Accessibility lets Parrot Core consume the shortcut and paste text. This Mac also needs Input Monitoring for shortcut listening." : "Microphone records dictation. Accessibility lets Parrot Core consume the shortcut and paste the finished text."}</p>
        </div>
        <div class="permission-list">
          ${renderPermissionRow("microphone", "Microphone", "Record your voice locally for dictation.", permissions.microphone, { variant: "setup" })}
          ${renderPermissionRow("accessibility", "Accessibility", "Consume the Parrot shortcut event and paste the finished text.", permissions.accessibility, { variant: "setup" })}
          ${
            showInputMonitoring
              ? renderPermissionRow("inputMonitoring", "Input Monitoring", "Some Macs require this so Parrot Core can listen for your shortcut while you use other apps.", permissions.inputMonitoring, { variant: "setup" })
              : ""
          }
        </div>
      </section>
    `;
  }

  if (setupStep === "language") {
    return `
      <section class="setup-section">
        <div class="section-heading">
          <h3>Which language(s) will you speak in?</h3>
          <p>English locales keep the fastest local path. Detect and other languages use the multilingual model.</p>
        </div>
        ${renderLanguageControls(current.settings, "setup")}
      </section>
    `;
  }

  return renderLocalModelsSection(current.settings, current.models, "setup");
}

function renderSetupActions(current: Snapshot) {
  const complete = setupRequirementsComplete(current);
  const currentIndex = setupStepIndex(setupStep);
  const previousButton =
    currentIndex > 0
      ? '<button id="setupBack" class="secondary" type="button">Back</button>'
      : "";

  if (setupStep !== "models") {
    const nextDisabled = setupStepComplete(current, setupStep)
      ? ""
      : 'disabled aria-disabled="true"';

    return `
      <footer class="setup-complete-actions">
        ${previousButton}
        <button id="setupNext" class="primary" type="button" ${nextDisabled}>Next</button>
      </footer>
    `;
  }

  const finishDisabled =
    complete && !setupFinalizing
      ? ""
      : `disabled aria-disabled="true" title="${inputMonitoringShownInOnboarding(current.settings) ? "Complete Microphone, Accessibility, Input Monitoring, and the required local models first" : "Complete Microphone, Accessibility, and the required local models first"}"`;
  const finishLabel = setupFinalizing ? "Preparing models..." : "Finish setup";

  return `
    <footer class="setup-complete-actions">
      ${previousButton}
      <button id="finishSetup" class="primary" type="button" ${finishDisabled}>${finishLabel}</button>
    </footer>
  `;
}

function renderSetupGate(current: Snapshot) {
  syncSetupStepWithSnapshot(current);
  const complete = setupRequirementsComplete(current);
  const showCompleteCopy = setupStep === "models" && complete;
  const meta = setupStepMeta(setupStep);

  return `
    <section class="setup-gate">
      <div class="setup-panel">
        ${renderSetupProgress()}

        <div class="setup-logo-wrap" aria-hidden="true">
          <span class="setup-logo-ring ring-1"></span>
          <span class="setup-logo-ring ring-2"></span>
          <span class="setup-logo-ring ring-3"></span>
          <span class="setup-logo-ring ring-4"></span>
          <img class="setup-logo" src="/logo_no_background.png?v=2" alt="" />
        </div>

        <header class="setup-header">
          <h1>${showCompleteCopy ? "Setup complete" : escapeHtml(meta.heading)}</h1>
          <p>
            ${
              showCompleteCopy
                ? "All required permissions and models are ready. Click Finish setup to start Parrot."
                : escapeHtml(meta.description)
            }
          </p>
        </header>

        ${toast ? renderToast(toast) : ""}

        <div class="setup-step-body">
          ${renderSetupStepContent(current)}
        </div>

        ${renderSetupActions(current)}
      </div>
    </section>
  `;
}

function renderPermissionRow(
  id: PermissionKind,
  title: string,
  description: string,
  state: PermissionState,
  options: PermissionRowOptions = {},
) {
  const granted = state === "granted";
  const setupVariant = options.variant === "setup";
  const opensSettings =
    (granted && options.openSettingsWhenGranted === true) ||
    ((id === "microphone" || id === "inputMonitoring") && state === "denied");

  const actionDisabled =
    granted && !opensSettings
      ? 'disabled aria-disabled="true"'
      : "";
  const actionLabel = opensSettings
    ? "Open settings"
    : granted
      ? "Enabled"
      : "Request";
  const showRefresh = !(options.hideRefreshWhenGranted && granted);

  return `
    <article class="permission-row ${granted ? "granted" : ""} ${setupVariant ? "setup-permission-row" : ""}">
      <div class="permission-status" aria-hidden="true">${setupVariant ? icon(id === "microphone" ? "mic" : id === "inputMonitoring" ? "keyboard" : "clipboard") : granted ? "✓" : "!"}</div>

      <div class="permission-copy">
        <div class="permission-title-line">
          <h3>${escapeHtml(title)}</h3>
        </div>
        <p>${escapeHtml(description)}</p>
      </div>

      <div class="permission-actions">
        <button
          class="small ${granted ? "secondary" : "primary"} request-permission"
          data-kind="${id}"
          data-open-settings="${opensSettings ? "true" : "false"}"
          ${actionDisabled}
        >
          ${escapeHtml(actionLabel)}
        </button>
        ${
          showRefresh
            ? `<button
          class="small secondary refresh-permission"
          data-kind="${id}"
        >
          Refresh
        </button>`
            : ""
        }
      </div>
    </article>
  `;
}

function scrollPageToTop() {
  requestAnimationFrame(() => {
    window.scrollTo({ top: 0, left: 0, behavior: "auto" });
    document.documentElement.scrollTop = 0;
    document.body.scrollTop = 0;
    const setupPanel = document.querySelector<HTMLElement>(".setup-panel");
    if (setupPanel) {
      setupPanel.scrollTop = 0;
      setupScrollTop = 0;
    }
  });
}

function restoreSetupScrollPosition() {
  requestAnimationFrame(() => {
    const setupPanel = document.querySelector<HTMLElement>(".setup-panel");
    if (!setupPanel) return;

    if (setupScrollTop <= 0) return;

    const maxTop = Math.max(
      0,
      setupPanel.scrollHeight - setupPanel.clientHeight,
    );
    const nextTop = Math.min(setupScrollTop, maxTop);

    if (Math.abs(setupPanel.scrollTop - nextTop) > 1) {
      setupPanel.scrollTop = nextTop;
    }
  });
}

async function applyMainWindowLayout(layout: "setup" | "main") {
  if (activeWindowLayout === layout) return;
  activeWindowLayout = layout;

  const window = getCurrentWindow();
  const size = layout === "setup" ? SETUP_WINDOW_SIZE : MAIN_WINDOW_SIZE;

  try {
    await window.setSize(size);
    await window.center();
  } catch (error) {
    console.warn("Could not resize Parrot window", error);
  }
}

function modelProgressState(model: ModelStatus) {
  const progressTotal = model.progressTotalBytes || model.expectedBytes;
  const progressBytes = model.downloading
    ? model.progressBytes
    : model.downloaded
      ? model.localBytes
      : 0;

  const progressPercent =
    progressTotal > 0
      ? Math.max(0, Math.min(100, (progressBytes / progressTotal) * 100))
      : 0;
  const finalizing =
    model.downloading && progressTotal > 0 && progressPercent >= 99.5;

  return { progressTotal, progressBytes, progressPercent, finalizing };
}

function syncModelFinalizingTimers(models: ModelStatus[]) {
  const activeFinalizing = new Set<string>();
  const now = Date.now();

  for (const model of models) {
    if (modelProgressState(model).finalizing) {
      activeFinalizing.add(model.id);
      if (!modelFinalizingStartedAt.has(model.id)) {
        modelFinalizingStartedAt.set(model.id, now);
      }
    }
  }

  for (const id of modelFinalizingStartedAt.keys()) {
    if (!activeFinalizing.has(id)) {
      modelFinalizingStartedAt.delete(id);
    }
  }
}

function finalizingModelDetail(modelId: string) {
  const startedAt = modelFinalizingStartedAt.get(modelId) || Date.now();
  const elapsedSeconds = Math.max(
    0,
    Math.floor((Date.now() - startedAt) / 1000),
  );
  const elapsed = `${formatElapsedSeconds(elapsedSeconds)} elapsed`;

  if (elapsedSeconds >= 60) {
    return `Still finalizing · ${elapsed}`;
  }
  return `Usually under 60 seconds · ${elapsed}`;
}

function formatElapsedSeconds(totalSeconds: number) {
  if (totalSeconds < 60) {
    return `${totalSeconds}s`;
  }

  const minutes = Math.floor(totalSeconds / 60);
  const seconds = totalSeconds % 60;
  return `${minutes}m ${seconds.toString().padStart(2, "0")}s`;
}

function modelStatusCopy(model: ModelStatus) {
  const { progressTotal, progressBytes, progressPercent, finalizing } =
    modelProgressState(model);

  const hasLocalData = modelHasLocalData(model);
  const modelSizeBytes = model.expectedBytes || model.localBytes;
  const status = model.downloading
    ? finalizing
      ? "Finalizing…"
      : "Downloading…"
    : model.downloaded
      ? "Downloaded"
      : hasLocalData
      ? "Incomplete"
      : "Not downloaded";

  const sizeLabel = formatBytes(modelSizeBytes);
  const progressLabel = model.downloading
    ? finalizing
      ? finalizingModelDetail(model.id)
      : `${formatBytes(progressBytes, "0 B")} / ${formatBytes(progressTotal)} · ${progressPercent.toFixed(0)}%`
    : sizeLabel;

  return { status, sizeLabel, progressLabel };
}

function modelHasLocalData(model: ModelStatus) {
  return model.localBytes > 0 || model.downloaded || Boolean(model.error);
}

function renderModelActionButton(model: ModelStatus) {
  if (model.downloading) {
    return `<button class="icon" disabled title="${escapeAttr(model.displayName)} is downloading" aria-label="${escapeAttr(model.displayName)} is downloading">${icon("download")}</button>`;
  }

  if (model.downloaded || modelHasLocalData(model)) {
    return `<button class="icon danger-icon delete-model" data-kind="${escapeAttr(model.id)}" title="Delete ${escapeAttr(model.displayName)}" aria-label="Delete ${escapeAttr(model.displayName)}">${icon("delete")}</button>`;
  }

  return `<button class="icon download-model" data-kind="${escapeAttr(model.id)}" title="Download ${escapeAttr(model.displayName)}" aria-label="Download ${escapeAttr(model.displayName)}">${icon("download")}</button>`;
}

function renderInlineModelProgress(model: ModelStatus) {
  if (!model.downloading) return "";

  const { progressPercent, finalizing } = modelProgressState(model);

  return `
    <div class="model-progress ${finalizing ? "finalizing" : ""}" role="progressbar" aria-valuemin="0" aria-valuemax="100" ${finalizing ? 'aria-label="Finalizing model download"' : `aria-valuenow="${progressPercent.toFixed(0)}"`}>
      <div class="model-progress-fill" style="width: ${progressPercent.toFixed(1)}%"></div>
    </div>
    <p class="model-progress-label">${escapeHtml(modelStatusCopy(model).progressLabel)}</p>
  `;
}

function renderLocalModelsSection(
  settings: AppSettings,
  models: ModelStatus[],
  context: "setup" | "general",
) {
  const cleanupHeading = context === "setup" ? "Cleanup (choose one)" : "Cleanup";
  const speechModelId =
    usesEnglishRoute(settings)
      ? MODEL_IDS.englishSpeech
      : MODEL_IDS.multilingualSpeech;
  const speechModel = models.find(
    (model) => model.id === speechModelId,
  );

  return `
    <section class="${context === "setup" ? "setup-section" : "settings-section"}">
      <div class="section-heading">
        <h3>Local models</h3>
        <p>These models run on your Mac. Downloaded models stay on disk until you delete them.</p>
      </div>

      <div class="local-model-stack">
        <div class="local-model-group">
          <div class="local-model-group-heading">
            <h3>Speech-to-text</h3>
            <p>Automatically selected from your language choice.</p>
          </div>
          ${speechModel ? renderSelectedSpeechModelCard(speechModel) : '<p class="empty">Speech model status is unavailable.</p>'}
        </div>

        <div class="local-model-group">
          <div class="local-model-group-heading">
            <h3>${cleanupHeading}</h3>
            <p>Choose how Parrot polishes raw dictation after transcription.</p>
          </div>
          ${renderCleanupModelChooser(settings, models, context)}
        </div>
      </div>
    </section>
  `;
}

function renderSelectedSpeechModelCard(model: ModelStatus) {
  const { sizeLabel } = modelStatusCopy(model);
  const classes = [
    "general-model-card",
    model.downloaded ? "downloaded" : "",
  ]
    .filter(Boolean)
    .join(" ");

  return `
    <article class="${classes}">
      <div class="general-model-card-main">
        <h3>${escapeHtml(model.displayName)}</h3>
        <p>${escapeHtml(model.subtitle)}</p>
        <div class="model-meta general-model-card-meta">
          <span>${escapeHtml(sizeLabel)}</span>
        </div>
      </div>
      <div class="general-model-card-action">
        ${renderModelActionButton(model)}
      </div>
      <div class="general-model-card-progress">${renderInlineModelProgress(model)}</div>
      ${model.error ? `<p class="model-error">${escapeHtml(model.error)}</p>` : ""}
    </article>
  `;
}

function renderCleanupModelCard(
  model: ModelStatus,
  context: "setup" | "general",
  selected: boolean,
) {
  const { sizeLabel } = modelStatusCopy(model);
  const notice =
    cleanupModelSelectionNotice?.id === model.id
      ? cleanupModelSelectionNotice.message
      : "";
  const classes = [
    "general-model-card",
    selected ? "active" : "",
    model.downloaded ? "downloaded" : "",
  ]
    .filter(Boolean)
    .join(" ");

  return `
    <article class="${classes}">
      <button
        class="general-model-card-main cleanup-model-select"
        data-cleanup-model-id="${escapeAttr(model.id)}"
        data-cleanup-model-context="${context}"
        role="radio"
        aria-checked="${selected ? "true" : "false"}"
      >
        <h3>${escapeHtml(model.displayName)}</h3>
        <p>${escapeHtml(model.subtitle)}</p>
        <div class="model-meta general-model-card-meta">
          <span>${escapeHtml(sizeLabel)}</span>
        </div>
      </button>
      <div class="general-model-card-action">
        ${renderModelActionButton(model)}
      </div>
      <div class="general-model-card-progress">${renderInlineModelProgress(model)}</div>
      ${model.error ? `<p class="model-error">${escapeHtml(model.error)}</p>` : ""}
      ${notice ? `<p class="general-model-card-notice" role="status">${escapeHtml(notice)}</p>` : ""}
    </article>
  `;
}

function cleanupModelsForSettings(models: ModelStatus[]) {
  return models.filter((model) => model.role === "cleanup");
}

function renderCleanupModelChooser(
  settings: AppSettings,
  models: ModelStatus[],
  context: "setup" | "general",
) {
  const selected = selectedCleanupModelId(settings);
  const options = cleanupModelsForSettings(models);
  const ariaLabel = context === "setup" ? "Cleanup model, choose one" : "Cleanup model";

  return `
    <div class="cleanup-model-grid general-layout" role="radiogroup" aria-label="${escapeAttr(ariaLabel)}">
      ${options
        .map((model) => {
          const isSelected = model.required || selected === model.id;
          return renderCleanupModelCard(model, context, isSelected);
        })
        .join("")}
    </div>
  `;
}

function renderRecording(settings: AppSettings, devices: AudioDevice[]) {
  return `
    <header><h2>Recording</h2><p>Choose a microphone and shortcuts.</p></header>
    <div class="card">
      <label class="field">
        <span>Microphone</span>
        <select id="inputDevice">${deviceOptions(devices, settings.selectedInputUid)}</select>
      </label>
    </div>
    ${renderShortcutSettings(settings)}
    <div class="card">
      <div class="row">
        <div>
          <h3>Test dictation</h3>
        </div>
        <button id="testDictation" class="primary">${testRecording ? "Stop test" : "Start test"}</button>
      </div>
      ${testResult ? `<pre class="result">${escapeHtml(testResult)}</pre>` : ""}
    </div>
    <div class="card compact">
      <label class="check"><input id="playSounds" type="checkbox" ${settings.playSounds ? "checked" : ""}/> Play dictation sounds</label>
    </div>
  `;
}

function renderShortcutSettings(settings: AppSettings) {
  const resetDisabled = shortcutCaptureTarget ? "disabled" : "";

  return `
    <div class="card shortcuts-card">
      <div class="shortcut-card-title">
        <div>
          <h3>Shortcuts</h3>
          <p>Use a hold shortcut for quick phrases, or a toggle shortcut for longer dictation.</p>
        </div>
      </div>

      ${shortcutNotice ? renderShortcutNotice(shortcutNotice) : ""}

      <div class="shortcut-list">
        ${renderShortcutSettingRow(
          "pushToTalkShortcut",
          "Push to talk",
          "Hold to record. Recording stops as soon as you release the shortcut.",
          settings.pushToTalkShortcut,
        )}
        ${renderShortcutSettingRow(
          "handsFreeShortcut",
          "Hands-free mode",
          "Press once to start recording. Press again to stop and paste.",
          settings.handsFreeShortcut,
        )}
      </div>

      <div class="button-row">
        <button id="resetShortcuts" class="secondary" ${resetDisabled}>Reset to defaults</button>
      </div>
    </div>
  `;
}

function renderShortcutNotice(notice: ShortcutNotice) {
  const role = notice.level === "error" ? "alert" : "status";
  const live = notice.level === "error" ? "assertive" : "polite";

  return `
    <div class="shortcut-notice ${notice.level} dismissible-alert" role="${role}" aria-live="${live}" aria-atomic="true">
      <span>${escapeHtml(notice.message)}</span>
      <button class="alert-close dismiss-shortcut-notice" type="button" aria-label="Dismiss">×</button>
    </div>
  `;
}

function renderToast(message: string) {
  return `
    <div class="toast dismissible-alert" role="status">
      <span>${escapeHtml(message)}</span>
      <button class="alert-close dismiss-toast" type="button" aria-label="Dismiss">×</button>
    </div>
  `;
}

function renderShortcutSettingRow(
  target: ShortcutSettingKey,
  title: string,
  description: string,
  shortcut: ShortcutSettings,
) {
  const capturing = shortcutCaptureTarget === target;
  const captureInProgress = shortcutCaptureTarget !== null;
  const preview = capturing ? "" : shortcut.displayName;
  const keycaps =
    shortcutKeycaps(preview) ||
    '<span class="shortcut-placeholder">Press shortcut…</span>';
  const help = capturing ? shortcutCaptureInlineHelp(target) : "";
  const recorderAriaLabel = capturing
    ? `Listening for ${title} shortcut`
    : `Change ${title} shortcut. Current shortcut: ${shortcut.displayName}`;
  const inlineHelp = capturing
    ? `<p class="shortcut-row-help" role="status" aria-live="polite" aria-atomic="true">${escapeHtml(help)}</p>`
    : "";
  const disabled = captureInProgress ? "disabled" : "";
  const shortcutEnabled = shortcut.enabled !== false;
  const settingDisabled = captureInProgress ? "disabled" : "";
  const doubleTapOption =
    target === "pushToTalkShortcut"
      ? `
        <label class="check shortcut-enable-row">
          <input
            class="shortcut-double-tap"
            data-shortcut-target="${target}"
            type="checkbox"
            ${shortcut.doubleTapToggle ? "checked" : ""}
            ${shortcutEnabled && !captureInProgress ? "" : "disabled"}
          />
          Double-tap to keep recording hands-free
        </label>
      `
      : "";

  return `
    <article class="shortcut-setting-row ${capturing ? "capturing" : ""} ${shortcutEnabled ? "" : "shortcut-disabled"}">
      <div class="shortcut-copy">
        <h3>${escapeHtml(title)}</h3>
        <p>${escapeHtml(description)}</p>
      </div>
      <div class="shortcut-controls">
        <button
          class="shortcut-recorder record-shortcut ${capturing ? "capturing" : ""}"
          data-shortcut-target="${target}"
          aria-pressed="${capturing ? "true" : "false"}"
          aria-label="${escapeAttr(recorderAriaLabel)}"
          ${disabled}
        >
          <span class="shortcut-recorder-label">${capturing ? "Listening for shortcut" : "Current shortcut"}</span>
          <span class="shortcut-display" aria-hidden="true">${keycaps}</span>
          <small>${capturing ? "Saves automatically" : "Click to change"}</small>
        </button>
        <label class="check shortcut-enable-row">
          <input
            class="shortcut-enabled"
            data-shortcut-target="${target}"
            type="checkbox"
            ${shortcutEnabled ? "checked" : ""}
            ${settingDisabled}
          />
          Enabled
        </label>
        ${doubleTapOption}
      </div>
      ${inlineHelp}
    </article>
  `;
}

function shortcutCaptureInlineHelp(target: ShortcutSettingKey) {
  if (target === "pushToTalkShortcut") {
    return "Press Fn, a single modifier, a modifier plus another key, or a function key. Press Escape to cancel.";
  }

  return "Press a modifier plus another key, or a function key. Single modifiers are ignored. Press Escape to cancel.";
}

function shortcutCaptureStartMessage(target: ShortcutSettingKey) {
  return `${shortcutTargetLabel(target)} is listening. Press the shortcut once. It saves automatically. Press Escape to cancel.`;
}

function shortcutKeycaps(displayName: string) {
  return displayName
    .split(" + ")
    .filter(Boolean)
    .map((part) => `<span class="keycap">${escapeHtml(part)}</span>`)
    .join("");
}

function renderCleanup(
  settings: AppSettings,
  models: ModelStatus[],
  defaultCleanupPrompt: string,
) {
  const cleanupModelId = selectedCleanupModelId(settings);
  const cleanup =
    models.find((m) => m.role === "cleanup" && m.required) ??
    models.find((m) => m.id === cleanupModelId);
  const cleanupDownloaded = Boolean(cleanup?.downloaded);
  const cleanupDownloading = Boolean(cleanup?.downloading);
  const cleanupChecked = settings.cleanupEnabled;
  const cleanupDisabled = cleanupDownloading;
  const cleanupHint = cleanupDownloading
    ? "Cleanup model is downloading. You can enable cleanup after it finishes."
    : cleanupDownloaded
      ? ""
      : "Download the selected cleanup model from General → Local models to use AI cleanup.";

  const routeDefaultPrompt = defaultCleanupPrompt;

  const hasCustomPrompt = settings.cleanupPrompt.trim().length > 0;
  const displayedPrompt = hasCustomPrompt
    ? settings.cleanupPrompt
    : routeDefaultPrompt;

  const promptHint = hasCustomPrompt
    ? "Using your custom cleanup prompt. Reset to restore default."
    : "Default cleanup prompt. Edit and save to customize it.";

  const dictionaryEntries = settings.dictionaryEntries || [];

  return `
    <header><h2>Cleanup</h2><p>Control cleanup and custom vocabulary.</p></header>

    <div class="card compact">
      <label class="check"><input id="cleanupEnabled" type="checkbox" ${cleanupChecked ? "checked" : ""} ${cleanupDisabled ? "disabled" : ""}/> Enable cleanup</label>
      ${cleanupHint ? `<p class="hint">${escapeHtml(cleanupHint)}</p>` : ""}
    </div>

    <div class="card">
      <label class="field">
        <span>Cleanup prompt</span>
        <textarea id="cleanupPrompt" class="prompt-editor" spellcheck="false">${escapeHtml(displayedPrompt)}</textarea>
      </label>
      <p class="hint">${escapeHtml(promptHint)}</p>
      <div class="button-row">
        <button id="saveCleanupPrompt" class="primary">Save prompt</button>
        <button id="resetCleanupPrompt" class="secondary">Reset to default</button>
      </div>
    </div>

    <div class="card">
      <div class="section-heading">
        <h3>Dictionary</h3>
        <p>Add names, project terms, acronyms, and uncommon phrases Parrot should preserve.</p>
      </div>

      <div class="dictionary-form">
        <input id="dictionaryTerm" class="text-input" placeholder="Word or phrase" maxlength="60" />
        <button id="addDictionaryEntry" class="primary">Add</button>
      </div>

      <div class="dictionary-list">
        ${dictionaryEntries.map(renderDictionaryRow).join("") || '<p class="empty">No Dictionary entries yet.</p>'}
      </div>
    </div>
  `;
}

function renderDictionaryRow(entry: DictionaryEntry) {
  return `
    <article class="dictionary-row">
      <h3>${escapeHtml(entry.term)}</h3>
      <button class="delete-dictionary icon danger-icon" data-id="${escapeAttr(entry.id)}" title="Delete dictionary entry" aria-label="Delete dictionary entry">${icon("delete")}</button>
    </article>
  `;
}

function renderHistory(settings: AppSettings, history: HistoryEntry[]) {
  return `
    <header><h2>History</h2><p>Optional local transcript archive.</p></header>
    <div class="card compact">
      <div class="row">
        <label class="check"><input id="historyEnabled" type="checkbox" ${settings.historyEnabled ? "checked" : ""}/> Save transcripts to history</label>
        <button id="clearHistory" class="danger" ${history.length === 0 ? "disabled" : ""}>Clear all</button>
      </div>
    </div>
    <div class="card">
      <input id="historySearch" class="search" placeholder="Search history" />
      <div id="historyList" class="history-list">
        ${history.map(renderHistoryRow).join("") || '<p class="empty">No saved recordings yet.</p>'}
      </div>
    </div>
  `;
}

function renderHistoryRow(entry: HistoryEntry) {
  const text =
    entry.cleanedTranscription || entry.rawTranscription || "No transcription";
  const date = formatDateTime(entry.createdAt);
  return `
    <article class="history-row" data-history-text="${escapeAttr(text.toLowerCase())}">
      <div>
        <h3>${escapeHtml(text)}</h3>
        <p>${escapeHtml(date)}</p>
      </div>
      <div class="history-actions">
        <button class="copy-history icon" data-transcript="${escapeAttr(text)}" title="Copy transcript" aria-label="Copy transcript">${icon("copy")}</button>
        <button class="delete-history icon danger-icon" data-id="${escapeAttr(entry.id)}" title="Delete transcript" aria-label="Delete transcript">${icon("delete")}</button>
      </div>
    </article>
  `;
}

function renderLanguageControls(
  settings: AppSettings,
  context: "setup" | "general",
) {
  const selectedLanguage = languageByCode(settings.dictationLanguageCode);
  const selectedLanguageCode =
    settings.dictationLanguageMode === "specific" && selectedLanguage
      ? selectedLanguage.code
      : (SPECIFIC_LANGUAGE_OPTIONS[0]?.code ?? "");
  const languagePicker =
    settings.dictationLanguageMode === "specific"
      ? `
      <label class="field language-picker" for="languageCode">
        <span>Specific language</span>
        <select id="languageCode">
          ${SPECIFIC_LANGUAGE_OPTIONS.map((language) =>
            renderLanguageSelectOption(language, selectedLanguageCode),
          ).join("")}
        </select>
      </label>
    `
      : "";
  const help =
    context === "setup"
      ? "For best accuracy, choose a specific language. Use Detect language if you switch languages between dictation sessions."
      : "Changing this updates which local speech and cleanup models Parrot requires.";

  return `
    <div class="language-panel">
      <div class="language-mode-grid" role="radiogroup" aria-label="Which language(s) will you speak in?">
        ${renderLanguageModeButton("english", "English", "Fastest local path", settings)}
        ${renderLanguageModeButton("detect", "Detect language", "For switching between sessions", settings)}
        ${renderLanguageModeButton("specific", "Specific language", "Best language or locale accuracy", settings)}
      </div>

      ${languagePicker}
      <p class="hint">${escapeHtml(help)}</p>
    </div>
  `;
}

function renderLanguageSelectOption(
  language: (typeof SPECIFIC_LANGUAGE_OPTIONS)[number],
  selectedCode: string,
) {
  const displayValue = languageDisplayValue(language);
  const selected = language.code === selectedCode;
  return `
    <option value="${escapeAttr(language.code)}" ${selected ? "selected" : ""}>
      ${escapeHtml(`${displayValue} - ${language.code}`)}
    </option>
  `;
}

function renderLanguageModeButton(
  mode: DictationLanguageMode,
  title: string,
  subtitle: string,
  settings: AppSettings,
) {
  const active = settings.dictationLanguageMode === mode;
  return `
    <button
      class="language-mode ${active ? "active" : ""}"
      data-language-mode="${mode}"
      aria-pressed="${active ? "true" : "false"}"
    >
      <span>${escapeHtml(title)}</span>
      <small>${escapeHtml(subtitle)}</small>
    </button>
  `;
}

function renderAbout() {
  return `
    <header>
      <h2>About</h2>
      <p>Version, license, credits, privacy, third-party licenses, and ways to help improve Parrot.</p>
    </header>

    <section class="card about-hero">
      <div class="about-hero-copy">
        <div class="about-title-line">
          <img class="about-logo" src="/logo_no_background.png?v=2" alt="" />
          <div>
            <h3>Parrot</h3>
            <p>Fast, free, open-source dictation.</p>
          </div>
        </div>
        <p class="about-description">
          Parrot turns your voice into clean text using local speech and cleanup models.
          Contributions, bug reports, language testing, docs, and platform work all help make it better.
        </p>
      </div>
      <div class="about-version" aria-label="Current Parrot version">
        <span>Version</span>
        <strong>${escapeHtml(APP_VERSION)}</strong>
      </div>
    </section>

    <section class="about-grid">
      <article class="about-card">
        <div class="about-card-icon">${icon("fileText")}</div>
        <h3>License</h3>
        <p>Parrot is released under the MIT License.</p>
        <div class="about-card-actions">
          ${renderExternalLinkButton("View license", LICENSE_URL)}
        </div>
      </article>

      <article class="about-card">
        <div class="about-card-icon">${icon("shield")}</div>
        <h3>Privacy</h3>
        <p>Parrot is local-first. Audio, transcripts, cleanup, settings, and dictionary entries stay on your Mac during normal use.</p>
        <div class="about-card-actions">
          ${renderExternalLinkButton("Read privacy notice", PRIVACY_URL)}
        </div>
      </article>

      <article class="about-card">
        <div class="about-card-icon">${icon("fileText")}</div>
        <h3>Credits</h3>
        <p>Parrot builds on open-source tools including WhisperKit, whisper.cpp, llama.cpp, Tauri, Swift, Rust, and TypeScript.</p>
        <div class="about-card-actions">
          ${renderExternalLinkButton("Third-party licenses", THIRD_PARTY_LICENSES_URL)}
        </div>
      </article>

      <article class="about-card">
        <div class="about-card-icon">${icon("star")}</div>
        <h3>Community</h3>
        <p>Bug reports, pull requests, ideas, language testing, and platform work are welcome.</p>
        <div class="about-card-actions">
          ${renderExternalLinkButton("Contributing guide", CONTRIBUTING_URL)}
        </div>
      </article>
    </section>

    <section class="settings-section">
      <div class="section-heading">
        <h3>Open source links</h3>
        <p>Report issues, suggest improvements, star the repo, or get in touch.</p>
      </div>
      <div class="about-action-grid">
        ${renderExternalLinkButton("View on GitHub", PARROT_REPO_URL, "primary", "externalLink")}
        ${renderExternalLinkButton("Report Bug", BUG_REPORT_URL, "secondary", "bug")}
        ${renderExternalLinkButton("Suggest Feature", FEATURE_REQUEST_URL, "secondary", "externalLink")}
        ${renderExternalLinkButton("Contributing Guide", CONTRIBUTING_URL, "secondary", "fileText")}
        ${renderExternalLinkButton("Privacy Notice", PRIVACY_URL, "secondary", "shield")}
        ${renderExternalLinkButton("Email", EMAIL_URL, "secondary", "mail")}
      </div>
    </section>
  `;
}

function renderExternalLinkButton(
  label: string,
  url: string,
  variant: "primary" | "secondary" = "secondary",
  iconName: IconName = "externalLink",
) {
  return `
    <button
      class="about-link-button ${variant} external-link"
      type="button"
      data-external-url="${escapeAttr(url)}"
    >
      <span>${icon(iconName)}</span>
      <span>${escapeHtml(label)}</span>
    </button>
  `;
}

function renderGeneral(
  settings: AppSettings,
  permissions: PermissionSnapshot,
  models: ModelStatus[],
) {
  return `
    <header><h2>General</h2><p>Settings, permissions, updates, and local models.</p></header>

    <div class="card compact">
      <label class="check"><input id="launchAtLogin" type="checkbox" ${settings.launchAtLogin ? "checked" : ""}/> Launch at login</label>
    </div>

    ${renderUpdateSection()}

    <section class="settings-section">
      <div class="section-heading">
        <h3>Permissions</h3>
        <p>Parrot needs these no matter which language or local model you use.</p>
      </div>
      <div class="permission-list embedded">
        ${renderPermissionRow("microphone", "Microphone", "Record your voice locally for dictation.", permissions.microphone, { hideRefreshWhenGranted: true, openSettingsWhenGranted: true })}
        ${renderPermissionRow("accessibility", "Accessibility", "Consume the Parrot shortcut event and paste the finished text.", permissions.accessibility, { hideRefreshWhenGranted: true, openSettingsWhenGranted: true })}
        ${
          shouldShowInputMonitoringPermission(settings)
            ? renderPermissionRow("inputMonitoring", "Input Monitoring", "Some Macs require this so Parrot Core can listen for your shortcut while you use other apps.", permissions.inputMonitoring, { hideRefreshWhenGranted: true, openSettingsWhenGranted: true })
            : ""
        }
      </div>
    </section>

    <section class="settings-section">
      <div class="section-heading">
        <h3>Dictation language</h3>
        <p>This chooses the speech-to-text route and which cleanup options are available.</p>
      </div>
      ${renderLanguageControls(settings, "general")}
    </section>

    ${renderLocalModelsSection(settings, models, "general")}
  `;
}

function renderUpdateSection() {
  const canInstall = updateStatus === "available" && updateInfo !== null;
  const checkDisabled = checkingForUpdate || installingUpdate ? "disabled" : "";
  const installDisabled = installingUpdate ? "disabled" : "";
  const notes = updateInfo ? updateNotesPreview(updateInfo.body ?? "") : "";
  const installLabel = updateInfo ? `Install ${updateInfo.version}` : "";

  return `
    <section class="settings-section">
      <div class="section-heading">
        <h3>Updates</h3>
        <p>Check for signed Parrot releases and install them when you choose.</p>
      </div>
      <article class="update-card ${updateCardStateClass()}">
        <div class="update-copy">
          <div class="update-title-line">
            <h3>Parrot updates</h3>
          </div>
          <p class="update-description" aria-live="polite">${escapeHtml(updateStatusDescription())}</p>
          ${renderUpdateInstallProgress()}
          ${
            notes
              ? `<div class="update-notes">
            <h4>Release notes</h4>
            <p>${escapeHtml(notes)}</p>
          </div>`
              : ""
          }
        </div>
        <div class="update-actions">
          <button id="checkForUpdate" class="small secondary" ${checkDisabled}>${checkingForUpdate ? "Checking…" : "Check for updates"}</button>
          ${
            canInstall
              ? `<button id="installUpdate" class="small primary" ${installDisabled}>${installingUpdate ? "Installing…" : escapeHtml(installLabel)}</button>`
              : ""
          }
        </div>
      </article>
    </section>
  `;
}

function renderUpdateInstallProgress() {
  if (!installingUpdate) return "";

  const progress = updateDownloadProgress;
  const totalBytes = progress?.totalBytes ?? null;
  const hasTotal = totalBytes !== null && totalBytes > 0;
  const percent =
    hasTotal && progress
      ? Math.max(0, Math.min(100, (progress.downloadedBytes / totalBytes) * 100))
      : null;

  return `
    <div
      class="update-progress ${hasTotal ? "" : "indeterminate"}"
      role="progressbar"
      aria-label="Update install progress"
      ${hasTotal ? `aria-valuemin="0" aria-valuemax="100" aria-valuenow="${percent!.toFixed(0)}"` : ""}
    >
      <div class="update-progress-fill" ${percent !== null ? `style="width: ${percent.toFixed(1)}%"` : ""}></div>
    </div>
    <p class="update-progress-label">${escapeHtml(updateProgressText())}</p>
  `;
}

function updateStatusDescription() {
  if (checkingForUpdate) return "Checking GitHub releases…";
  if (installingUpdate)
    return "Downloading and installing the selected update. Parrot will relaunch when it finishes.";
  if (updateStatus === "available" && updateInfo)
    return `Parrot ${updateInfo.version} is available. Install it when you're ready.`;
  if (updateStatus === "current") return "Parrot is up to date.";
  if (updateStatus === "error")
    return "Update check failed. Try again when you're online.";
  return "Check manually for a new Parrot release. Updates never install automatically.";
}

function updateCardStateClass() {
  return updateStatus === "error" ? "error" : "";
}

function updateProgressText() {
  if (updateDownloadFinished) return "Installing update…";

  const progress = updateDownloadProgress;
  if (!progress) return "Preparing update download…";
  if (progress.totalBytes && progress.totalBytes > 0) {
    return `Downloading ${formatBytes(progress.downloadedBytes)} of ${formatBytes(progress.totalBytes)}…`;
  }
  if (progress.downloadedBytes > 0) {
    return `Downloaded ${formatBytes(progress.downloadedBytes)}…`;
  }
  return "Downloading update…";
}

function updateNotesPreview(notes: string) {
  const trimmed = notes.trim();
  if (trimmed.length <= 1400) return trimmed;
  return `${trimmed.slice(0, 1400).trimEnd()}...`;
}

function acceptPermissionSnapshot(permissions: PermissionSnapshot) {
  if (!snapshot) return;
  snapshot = { ...snapshot, permissions };
}

function bindPermissionButtons() {
  document
    .querySelectorAll<HTMLButtonElement>(".request-permission")
    .forEach((button) => {
      button.onclick = async () => {
        const kind = button.dataset.kind as PermissionKind | undefined;
        const openSettings = button.dataset.openSettings === "true";

        if (!kind || !snapshot) return;

        toast = "";

        try {
          const permissions = await invoke<PermissionSnapshot>(
            "request_permission",
            { kind, openSettings },
          );
          acceptPermissionSnapshot(permissions);
        } catch (error) {
          toast = `Could not request permission: ${errorMessage(error)}`;
        }

        render();
      };
    });

  document
    .querySelectorAll<HTMLButtonElement>(".refresh-permission")
    .forEach((button) => {
      button.onclick = async () => {
        await refreshPermissionStatus();
      };
    });
}

function bindModelButtons() {
  document
    .querySelectorAll<HTMLButtonElement>(".download-model")
    .forEach((button) => {
      button.onclick = async () => {
        const kind = button.dataset.kind;
        if (!kind) return;

        if (cleanupModelSelectionNotice?.id === kind) {
          cleanupModelSelectionNotice = null;
        }
        toast = "";
        try {
          snapshot = await invoke<Snapshot>("download_model", { kind });
          render();
          pollModelsUntilStable();
        } catch (error) {
          toast = `Could not start model download: ${errorMessage(error)}`;
          render();
        }
      };
    });

  document
    .querySelectorAll<HTMLButtonElement>(".delete-model")
    .forEach((button) => {
      button.onclick = async () => {
        const kind = button.dataset.kind;
        if (!kind) return;

        toast = "Deleting model…";
        render();

        try {
          snapshot = await invoke<Snapshot>("delete_model", { kind });
          if (
            snapshot.models.every((model) => !model.downloading) &&
            modelPollHandle !== null
          ) {
            window.clearInterval(modelPollHandle);
            modelPollHandle = null;
          }
          toast = "";
        } catch (error) {
          toast = `Could not delete model: ${errorMessage(error)}`;
        }

        render();
      };
    });
}

function bindCleanupModelChooser() {
  document
    .querySelectorAll<HTMLButtonElement>(".cleanup-model-select")
    .forEach((button) => {
      button.onclick = async () => {
        if (!snapshot) return;

        const id = button.dataset.cleanupModelId as CleanupModelId | undefined;
        if (!id) return;

        if (button.dataset.cleanupModelContext === "general") {
          const model = snapshot.models.find((candidate) => candidate.id === id);
          if (!model?.downloaded) {
            cleanupModelSelectionNotice = {
              id,
              message: "Download this model before selecting it.",
            };
            toast = "";
            render();
            return;
          }
        }

        cleanupModelSelectionNotice = null;
        toast = "";

        await saveSettings({
          cleanupModelId: id,
        });
      };
    });
}

function bindLanguageControls() {
  document
    .querySelectorAll<HTMLButtonElement>("[data-language-mode]")
    .forEach((button) => {
      button.onclick = async () => {
        const mode = button.dataset.languageMode as
          | DictationLanguageMode
          | undefined;
        if (!mode) return;

        if (mode === "specific") {
          const fallback =
            languageByCode(snapshot?.settings.dictationLanguageCode) ||
            SPECIFIC_LANGUAGE_OPTIONS.find((language) => language.code === "es") ||
            SPECIFIC_LANGUAGE_OPTIONS[0];
          await saveLanguageChoice("specific", fallback.code);
          return;
        }

        await saveLanguageChoice(mode, null);
      };
    });

  const languageCode = document.querySelector<HTMLSelectElement>("#languageCode");
  if (languageCode) {
    languageCode.onchange = () => {
      void saveLanguageChoice("specific", languageCode.value);
    };
  }
}

async function saveLanguageChoice(
  mode: DictationLanguageMode,
  code: string | null,
) {
  if (!snapshot) return;
  toast = "";
  const next = await invoke<Snapshot>("save_settings", {
    settings: {
      ...snapshot.settings,
      dictationLanguageMode: mode,
      dictationLanguageCode: code,
    },
  });
  snapshot = next;

  render();
}

function bindSetupEvents() {
  bindAlertDismissers();
  bindPermissionButtons();
  bindModelButtons();
  bindCleanupModelChooser();
  bindLanguageControls();

  const setupBack = document.querySelector<HTMLButtonElement>("#setupBack");
  if (setupBack)
    setupBack.onclick = () => {
      const index = setupStepIndex(setupStep);
      if (index <= 0) return;
      setupStep = SETUP_STEPS[index - 1].id;
      toast = "";
      render();
      scrollPageToTop();
    };

  const setupNext = document.querySelector<HTMLButtonElement>("#setupNext");
  if (setupNext)
    setupNext.onclick = () => {
      if (!snapshot || !setupStepComplete(snapshot, setupStep)) return;
      const index = setupStepIndex(setupStep);
      if (index >= SETUP_STEPS.length - 1) return;
      setupStep = SETUP_STEPS[index + 1].id;
      toast = "";
      render();
      scrollPageToTop();
    };

  const finishSetup = document.querySelector<HTMLButtonElement>("#finishSetup");
  if (finishSetup)
    finishSetup.onclick = async () => {
      if (!snapshot) return;

      try {
        const permissions = await invoke<PermissionSnapshot>(
          "permission_statuses",
        );
        acceptPermissionSnapshot(permissions);

        if (!snapshot || !setupRequirementsComplete(snapshot)) {
          toast =
            inputMonitoringShownInOnboarding(snapshot?.settings)
              ? "Setup is not complete yet. Enable Microphone, Accessibility, and Input Monitoring, then download the required models first."
              : "Setup is not complete yet. Enable Microphone and Accessibility, then download the required models first.";
          render();
          return;
        }

        setupFinalizing = true;
        toast = "Preparing local models for first use…";
        render();

        await invoke("warm_models");
        await invoke("set_hotkey_monitor_enabled", { enabled: true });
        snapshot = await invoke<Snapshot>("save_settings", {
          settings: { ...snapshot.settings, onboardingCompleted: true },
        });

        setupFinalizing = false;
        toast = "";
        render();
      } catch (error) {
        const message = errorMessage(error);
        await maybeRequireInputMonitoring(message);
        setupFinalizing = false;
        toast = `Setup is almost complete, but Parrot could not start the shortcut monitor: ${message}`;
        render();
      }
    };
}

async function refreshPermissionStatus(options: { silent?: boolean } = {}) {
  if (!snapshot) return;

  if (!options.silent) toast = "";

  try {
    const permissions = await invoke<PermissionSnapshot>("permission_statuses");
    acceptPermissionSnapshot(permissions);
  } catch (error) {
    if (!options.silent) {
      toast = `Could not refresh permissions: ${errorMessage(error)}`;
    }
  }

  render();
}

function syncSetupPolling() {
  const shouldPoll =
    snapshot !== null &&
    !setupPermissionsComplete(snapshot.permissions, snapshot.settings);

  if (shouldPoll && setupPollHandle === null) {
    setupPollHandle = window.setInterval(() => {
      void refreshPermissionStatus({ silent: true });
    }, 2500);
  }

  if (!shouldPoll && setupPollHandle !== null) {
    window.clearInterval(setupPollHandle);
    setupPollHandle = null;
  }
}

function bindEvents() {
  bindAlertDismissers();
  bindPermissionButtons();
  bindModelButtons();
  bindCleanupModelChooser();
  bindLanguageControls();
  bindUpdateButtons();
  bindExternalLinks();

  document
    .querySelectorAll<HTMLButtonElement>("[data-tab]")
    .forEach((button) => {
      button.onclick = () => {
        if (shortcutCaptureTarget) return;
        selectMainTab(button.dataset.tab as MainTab);
      };
    });

  const inputDevice = document.querySelector<HTMLSelectElement>("#inputDevice");
  if (inputDevice)
    inputDevice.onchange = () => {
      saveSettings({
        selectedInputUid: inputDevice.value || null,
      });
    };

  const playSounds = document.querySelector<HTMLInputElement>("#playSounds");
  if (playSounds)
    playSounds.onchange = () =>
      saveSettings({ playSounds: playSounds.checked });

  const cleanupEnabled =
    document.querySelector<HTMLInputElement>("#cleanupEnabled");
  if (cleanupEnabled)
    cleanupEnabled.onchange = () => {
      saveSettings({ cleanupEnabled: cleanupEnabled.checked });
    };

  const saveCleanupPrompt =
    document.querySelector<HTMLButtonElement>("#saveCleanupPrompt");
  if (saveCleanupPrompt)
    saveCleanupPrompt.onclick = async () => {
      if (!snapshot) return;

      const rawPrompt =
        document.querySelector<HTMLTextAreaElement>("#cleanupPrompt")?.value ?? "";

      const routeDefaultPrompt = snapshot.defaultCleanupPrompt;

      const shouldUseDefault =
        rawPrompt.trim().length === 0 ||
        rawPrompt.trim() === routeDefaultPrompt.trim();

      toast = "";

      await saveSettings({
        cleanupPrompt: shouldUseDefault ? "" : rawPrompt,
      });
    };

  const resetCleanupPrompt =
    document.querySelector<HTMLButtonElement>("#resetCleanupPrompt");
  if (resetCleanupPrompt)
    resetCleanupPrompt.onclick = async () => {
      toast = "";
      await saveSettings({ cleanupPrompt: "" });
    };

  const addDictionaryEntry =
    document.querySelector<HTMLButtonElement>("#addDictionaryEntry");
  if (addDictionaryEntry)
    addDictionaryEntry.onclick = () => {
      if (!snapshot) return;

      const term =
        document.querySelector<HTMLInputElement>("#dictionaryTerm")?.value ??
        "";
      const entry = dictionaryEntryFromInput(term);

      if (!entry) {
        render();
        return;
      }

      toast = "";
      saveSettings({
        dictionaryEntries: mergeDictionaryEntry(
          snapshot.settings.dictionaryEntries || [],
          entry,
        ),
      });
    };

  document
    .querySelectorAll<HTMLButtonElement>(".delete-dictionary")
    .forEach((button) => {
      button.onclick = () => {
        if (!snapshot) return;
        const id = button.dataset.id;

        saveSettings({
          dictionaryEntries: (
            snapshot.settings.dictionaryEntries || []
          ).filter((entry) => entry.id !== id),
        });
      };
    });

  const historyEnabled =
    document.querySelector<HTMLInputElement>("#historyEnabled");
  if (historyEnabled)
    historyEnabled.onchange = () =>
      saveSettings({ historyEnabled: historyEnabled.checked });

  const launchAtLogin =
    document.querySelector<HTMLInputElement>("#launchAtLogin");
  if (launchAtLogin)
    launchAtLogin.onchange = async () => {
      toast = "";
      try {
        snapshot = await invoke<Snapshot>("set_launch_at_login", {
          enabled: launchAtLogin.checked,
        });
        render();
      } catch (error) {
        toast = `Could not update launch at login: ${errorMessage(error)}`;
        await load();
      }
    };

  const testDictation =
    document.querySelector<HTMLButtonElement>("#testDictation");
  if (testDictation)
    testDictation.onclick = async () => {
      if (!testRecording) {
        testRecording = true;
        testResult = "Recording… speak now, then click Stop test.";
        try {
          await invoke("start_test_dictation");
        } catch (error) {
          testRecording = false;
          testResult = `Could not start recording: ${errorMessage(error)}`;
        }
        render();
      } else {
        testRecording = false;
        testResult = "Transcribing…";
        render();
        try {
          const result = await invoke<{ raw: string; cleaned: string }>(
            "stop_test_dictation",
          );
          testResult = result.cleaned || result.raw;
          await load();
        } catch (error) {
          testResult = `Could not transcribe recording: ${errorMessage(error)}`;
          render();
        }
      }
    };

  const clearHistory =
    document.querySelector<HTMLButtonElement>("#clearHistory");
  if (clearHistory)
    clearHistory.onclick = () => {
      clearHistoryBusy = false;
      confirmClearHistoryOpen = true;
      toast = "";
      render();
      document.querySelector<HTMLButtonElement>("#cancelClearHistory")?.focus();
    };

  const cancelClearHistory = document.querySelector<HTMLButtonElement>(
    "#cancelClearHistory",
  );
  if (cancelClearHistory)
    cancelClearHistory.onclick = () => {
      if (clearHistoryBusy) return;
      confirmClearHistoryOpen = false;
      toast = "";
      render();
    };

  const confirmClearHistory = document.querySelector<HTMLButtonElement>(
    "#confirmClearHistory",
  );
  if (confirmClearHistory)
    confirmClearHistory.onclick = async () => {
      if (clearHistoryBusy) return;

      clearHistoryBusy = true;
      confirmClearHistory.disabled = true;
      confirmClearHistory.textContent = "Clearing…";
      if (cancelClearHistory) {
        cancelClearHistory.disabled = true;
      }

      try {
        snapshot = await invoke<Snapshot>("clear_history");
        toast = "";
      } catch (error) {
        toast = `Could not clear history: ${errorMessage(error)}`;
      } finally {
        clearHistoryBusy = false;
        confirmClearHistoryOpen = false;
        render();
      }
    };

  document
    .querySelectorAll<HTMLButtonElement>(".copy-history")
    .forEach((button) => {
      button.onclick = async () => {
        try {
          await navigator.clipboard.writeText(button.dataset.transcript || "");
          toast = "";
          document.querySelector<HTMLElement>(".content > .toast")?.remove();
          showHistoryCopyFeedback(button);
        } catch (error) {
          toast = `Could not copy transcript: ${errorMessage(error)}`;
          render();
        }
      };
    });

  document
    .querySelectorAll<HTMLButtonElement>(".delete-history")
    .forEach((button) => {
      button.onclick = async () => {
        try {
          snapshot = await invoke<Snapshot>("delete_history_item", {
            id: button.dataset.id,
          });
        } catch (error) {
          toast = `Could not delete history item: ${errorMessage(error)}`;
        }
        render();
      };
    });

  const search = document.querySelector<HTMLInputElement>("#historySearch");
  if (search)
    search.oninput = () => {
      const query = search.value.toLowerCase();
      document.querySelectorAll<HTMLElement>(".history-row").forEach((row) => {
        row.style.display = row.dataset.historyText?.includes(query)
          ? ""
          : "none";
      });
    };

  document
    .querySelectorAll<HTMLButtonElement>(".record-shortcut")
    .forEach((button) => {
      button.onclick = async () => {
        const target = button.dataset.shortcutTarget as
          | ShortcutSettingKey
          | undefined;
        if (!target) return;
        await beginShortcutCapture(target);
      };
    });

  document
    .querySelectorAll<HTMLInputElement>(".shortcut-enabled")
    .forEach((input) => {
      input.onchange = async () => {
        if (!snapshot) return;
        const target = input.dataset.shortcutTarget as ShortcutSettingKey;
        const current = snapshot.settings[target];

        await saveSettings({
          [target]: {
            ...current,
            enabled: input.checked,
          },
        } as Partial<AppSettings>);
      };
    });

  document
    .querySelectorAll<HTMLInputElement>(".shortcut-double-tap")
    .forEach((input) => {
      input.onchange = async () => {
        if (!snapshot) return;
        const target = input.dataset.shortcutTarget as ShortcutSettingKey;
        const current = snapshot.settings[target];

        await saveSettings({
          [target]: {
            ...current,
            doubleTapToggle: input.checked,
          },
        } as Partial<AppSettings>);
      };
    });

  const resetShortcuts =
    document.querySelector<HTMLButtonElement>("#resetShortcuts");
  if (resetShortcuts)
    resetShortcuts.onclick = async () => {
      const shouldRestart =
        shortcutMonitorPausedForCapture && snapshot?.permissions.allGranted;
      shortcutCaptureSession += 1;
      shortcutCaptureTarget = null;
      shortcutMonitorPausedForCapture = false;
      shortcutNotice = {
        level: "success",
        message: "Shortcuts reset to defaults.",
      };
      toast = "";
      await saveSettings({
        pushToTalkShortcut: {
          ...DEFAULT_PUSH_TO_TALK_SHORTCUT,
          macosKeyCodes: [...DEFAULT_PUSH_TO_TALK_SHORTCUT.macosKeyCodes],
        },
        handsFreeShortcut: {
          ...DEFAULT_HANDS_FREE_SHORTCUT,
          macosKeyCodes: [...DEFAULT_HANDS_FREE_SHORTCUT.macosKeyCodes],
        },
      });
      if (shouldRestart) {
        try {
          await invoke("set_hotkey_monitor_enabled", { enabled: true });
        } catch (error) {
          const message = errorMessage(error);
          await maybeRequireInputMonitoring(message);
          shortcutNotice = {
            level: "error",
            message: `Shortcuts reset, but the hotkey monitor did not restart: ${message}`,
          };
          render();
        }
      }
    };
}

function bindAlertDismissers() {
  document.querySelectorAll<HTMLButtonElement>(".dismiss-toast").forEach((button) => {
    button.onclick = () => {
      toast = "";
      render();
    };
  });

  document
    .querySelectorAll<HTMLButtonElement>(".dismiss-shortcut-notice")
    .forEach((button) => {
      button.onclick = () => {
        shortcutNotice = null;
        render();
      };
    });
}

function bindUpdateButtons() {
  const checkForUpdate =
    document.querySelector<HTMLButtonElement>("#checkForUpdate");
  if (checkForUpdate)
    checkForUpdate.onclick = () => {
      void checkForUpdatesManually();
    };

  const installUpdate =
    document.querySelector<HTMLButtonElement>("#installUpdate");
  if (installUpdate)
    installUpdate.onclick = () => {
      void installParrotUpdate();
    };
}

function bindExternalLinks() {
  document.querySelectorAll<HTMLButtonElement>(".external-link").forEach((button) => {
    button.onclick = async () => {
      const url = button.dataset.externalUrl;
      if (!url) return;

      try {
        await openUrl(url);
      } catch (error) {
        toast = `Could not open link: ${errorMessage(error)}`;
        render();
      }
    };
  });
}

function showHistoryCopyFeedback(button: HTMLButtonElement) {
  const previousTimer = historyCopyFeedbackTimers.get(button);
  if (previousTimer !== undefined) {
    window.clearTimeout(previousTimer);
  }

  button.classList.add("copied");
  button.innerHTML = icon("check");
  button.title = "Copied";
  button.setAttribute("aria-label", "Copied");

  const timer = window.setTimeout(() => {
    button.classList.remove("copied");
    button.innerHTML = icon("copy");
    button.title = "Copy transcript";
    button.setAttribute("aria-label", "Copy transcript");
    historyCopyFeedbackTimers.delete(button);
  }, 1200);

  historyCopyFeedbackTimers.set(button, timer);
}

async function beginShortcutCapture(target: ShortcutSettingKey) {
  if (!snapshot || shortcutCaptureTarget) return;

  const session = ++shortcutCaptureSession;
  shortcutCaptureTarget = target;
  shortcutMonitorPausedForCapture = snapshot.permissions.allGranted;
  shortcutNotice = {
    level: "info",
    message: shortcutCaptureStartMessage(target),
  };
  toast = "";
  render();

  try {
    const shortcut = await invoke<ShortcutSettings>("capture_shortcut", {
      target,
    });

    if (session !== shortcutCaptureSession || !snapshot) return;

    const otherShortcut =
      target === "pushToTalkShortcut"
        ? snapshot.settings.handsFreeShortcut
        : snapshot.settings.pushToTalkShortcut;
    if (shortcutsEquivalent(shortcut, otherShortcut)) {
      await finishShortcutCapture(
        "Push to talk and hands-free mode need different shortcuts.",
        "error",
      );
      return;
    }

    await saveSettings({
      [target]: {
        ...snapshot.settings[target],
        displayName: shortcut.displayName,
        macosKeyCodes: shortcut.macosKeyCodes,
        mode: shortcut.mode,
      },
    } as Partial<AppSettings>);

    await finishShortcutCapture(
      `${shortcutTargetLabel(target)} shortcut saved: ${shortcut.displayName}`,
      "success",
    );
  } catch (error) {
    const message = errorMessage(error) || "Shortcut capture cancelled.";
    await finishShortcutCapture(
      message,
      message.toLowerCase().includes("cancelled") ? "info" : "error",
    );
  }
}

async function finishShortcutCapture(
  message: string,
  level: ShortcutNotice["level"] = "success",
) {
  const shouldRestart = shortcutMonitorPausedForCapture;
  shortcutCaptureSession += 1;
  shortcutCaptureTarget = null;
  shortcutMonitorPausedForCapture = false;

  if (!snapshot?.permissions.allGranted) {
    shortcutNotice = {
      level,
      message: `${message} Finish setup to enable the shortcut.`,
    };
    render();
    return;
  }

  if (shouldRestart) {
    try {
      await invoke("set_hotkey_monitor_enabled", { enabled: true });
    } catch (error) {
      const restartMessage = errorMessage(error);
      await maybeRequireInputMonitoring(restartMessage);
      shortcutNotice = {
        level: "error",
        message: `${message} Hotkey monitor did not restart: ${restartMessage}`,
      };
      render();
      return;
    }
  }

  shortcutNotice = { level, message };
  render();
}

function shortcutTargetLabel(target: ShortcutSettingKey) {
  return target === "pushToTalkShortcut" ? "Push to talk" : "Hands-free mode";
}

function shortcutsEquivalent(a: ShortcutSettings, b: ShortcutSettings) {
  return shortcutCodeKey(a) === shortcutCodeKey(b);
}

function shortcutCodeKey(shortcut: ShortcutSettings) {
  return [...new Set(shortcut.macosKeyCodes)]
    .sort((left, right) => left - right)
    .join(",");
}

function dictionaryEntryFromInput(term: string): DictionaryEntry | null {
  const cleanTerm = collapseWhitespace(term);

  if (!cleanTerm) {
    toast = "Add a Dictionary word or phrase first.";
    return null;
  }

  if (cleanTerm.length > 60) {
    toast = "Dictionary words and phrases must be 60 characters or fewer.";
    return null;
  }

  return {
    id: newDictionaryId(),
    term: cleanTerm,
  };
}

function mergeDictionaryEntry(
  entries: DictionaryEntry[],
  entry: DictionaryEntry,
): DictionaryEntry[] {
  const key = dictionaryEntryKey(entry);
  const existing = entries.find(
    (candidate) => dictionaryEntryKey(candidate) === key,
  );

  if (!existing) return [...entries, entry];

  return entries.map((candidate) =>
    dictionaryEntryKey(candidate) === key
      ? {
          ...candidate,
          ...entry,
          id: candidate.id,
        }
      : candidate,
  );
}

function dictionaryEntryKey(entry: DictionaryEntry) {
  return collapseWhitespace(entry.term).toLowerCase();
}

function collapseWhitespace(value: string) {
  return value.replace(/\s+/g, " ").trim();
}

function newDictionaryId() {
  return (
    globalThis.crypto?.randomUUID?.() ??
    `${Date.now()}-${Math.random().toString(36).slice(2)}`
  );
}

function pollModelsUntilStable() {
  if (modelPollHandle !== null) window.clearInterval(modelPollHandle);
  modelPollHandle = window.setInterval(async () => {
    try {
      snapshot = await invoke<Snapshot>("get_app_snapshot");
      render();
      const stillDownloading = snapshot.models.some(
        (model) => model.downloading,
      );
      if (!stillDownloading && modelPollHandle !== null) {
        window.clearInterval(modelPollHandle);
        modelPollHandle = null;
        toast = snapshot.models.some((model) => model.error)
          ? "A model download failed."
          : "";
        render();
      }
    } catch (error) {
      if (modelPollHandle !== null) window.clearInterval(modelPollHandle);
      modelPollHandle = null;
      toast = `Could not refresh model status: ${errorMessage(error)}`;
      render();
    }
  }, 1500);
}

function formatDateTime(value: string) {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return date.toLocaleString();
}

function formatBytes(bytes: number, zeroLabel = "—") {
  if (!Number.isFinite(bytes) || bytes < 1) return zeroLabel;
  const units = ["B", "KB", "MB", "GB"];
  let value = bytes;
  let unit = 0;
  while (value >= 1000 && unit < units.length - 1) {
    value /= 1000;
    unit += 1;
  }
  return `${value.toFixed(value >= 10 || unit === 0 ? 0 : 1)} ${units[unit]}`;
}

function escapeHtml(value: string) {
  return value.replace(
    /[&<>'"]/g,
    (ch) =>
      ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", "'": "&#39;", '"': "&quot;" })[
        ch
      ]!,
  );
}
function escapeAttr(value: string) {
  return escapeHtml(value);
}
function errorMessage(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}
function friendlyUpdateError(error: unknown) {
  const message = errorMessage(error).trim();
  return message
    ? `${message}. Try again when you're online or from an installed Parrot release.`
    : "Try again when you're online or from an installed Parrot release.";
}
function icon(name: IconName) {
  return ICONS[name];
}

boot().catch((error) => {
  app.innerHTML = `<pre class="error">${escapeHtml(String(error))}</pre>`;
});
