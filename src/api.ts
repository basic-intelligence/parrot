import { invoke } from "@tauri-apps/api/core";
import type {
  AppSettings,
  DictationResult,
  ModelStatus,
  PermissionKind,
  PermissionSnapshot,
  RecordingResultPayload,
  ShortcutSettings,
  Snapshot,
} from "./types";

export function getAppSnapshot() {
  return invoke<Snapshot>("get_app_snapshot");
}

export function saveSettings(settings: AppSettings) {
  return invoke<Snapshot>("save_settings", { settings });
}

export function saveRecordingResult(result: RecordingResultPayload) {
  return invoke<Snapshot>("save_recording_result", { result });
}

export function setUpdateBadge(available: boolean, version: string | null) {
  return invoke<void>("set_update_badge", { available, version });
}

export function downloadModel(kind: string) {
  return invoke<ModelStatus[]>("download_model", { kind });
}

export function modelStatuses() {
  return invoke<ModelStatus[]>("model_statuses");
}

export function deleteModel(kind: string) {
  return invoke<Snapshot>("delete_model", { kind });
}

export function requestPermission(kind: PermissionKind, openSettings: boolean) {
  return invoke<PermissionSnapshot>("request_permission", {
    kind,
    openSettings,
  });
}

export function permissionStatuses() {
  return invoke<PermissionSnapshot>("permission_statuses");
}

export function warmModels() {
  return invoke<void>("warm_models");
}

export function setHotkeyMonitorEnabled(enabled: boolean) {
  return invoke<void>("set_hotkey_monitor_enabled", { enabled });
}

export function setLaunchAtLogin(enabled: boolean) {
  return invoke<Snapshot>("set_launch_at_login", { enabled });
}

export function startTestDictation() {
  return invoke<void>("start_test_dictation");
}

export function stopTestDictation() {
  return invoke<DictationResult>("stop_test_dictation");
}

export function clearHistory() {
  return invoke<Snapshot>("clear_history");
}

export function deleteHistoryItem(id: string) {
  return invoke<Snapshot>("delete_history_item", { id });
}

export function captureShortcut(target: string) {
  return invoke<ShortcutSettings>("capture_shortcut", { target });
}

export function installLinuxShortcuts() {
  return invoke<void>("install_linux_shortcuts");
}
