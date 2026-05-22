use anyhow::anyhow;
use parrot_protocol::{PermissionKind, PermissionRequirement, PermissionSnapshot, PermissionState};

#[derive(Debug, Default)]
pub struct PermissionManager;

impl PermissionManager {
    pub fn statuses(&self, selected_input_uid: Option<&str>) -> PermissionSnapshot {
        microphone_snapshot(microphone_state(selected_input_uid))
    }

    pub fn request_permission(
        &self,
        kind: PermissionKind,
        open_settings: bool,
        selected_input_uid: Option<&str>,
    ) -> anyhow::Result<PermissionSnapshot> {
        if kind != PermissionKind::Microphone {
            return Err(anyhow!(
                "Windows permission `{kind:?}` is not requestable by native-core"
            ));
        }

        if open_settings {
            open_microphone_settings()?;
        }

        Ok(self.statuses(selected_input_uid))
    }
}

fn microphone_snapshot(state: PermissionState) -> PermissionSnapshot {
    let granted = state == PermissionState::Granted;
    PermissionSnapshot {
        requirements: vec![PermissionRequirement {
            kind: PermissionKind::Microphone,
            title: "Microphone".into(),
            description: "Record your voice locally for dictation.".into(),
            state: state.clone(),
            required: true,
            requestable: true,
            opens_settings: true,
        }],
        all_required_granted: granted,
        microphone: Some(state),
        accessibility: None,
        input_monitoring: None,
        all_granted: Some(granted),
    }
}

#[cfg(target_os = "windows")]
fn microphone_state(selected_input_uid: Option<&str>) -> PermissionState {
    microphone_state_from_probe_result(crate::platform::audio::probe_input_device(
        selected_input_uid,
    ))
}

#[cfg(not(target_os = "windows"))]
fn microphone_state(_selected_input_uid: Option<&str>) -> PermissionState {
    PermissionState::Unknown
}

fn microphone_state_from_probe_result(result: anyhow::Result<()>) -> PermissionState {
    match result {
        Ok(()) => PermissionState::Granted,
        Err(error) if is_known_microphone_denial(&error.to_string()) => PermissionState::Denied,
        Err(_) => PermissionState::Unknown,
    }
}

fn is_known_microphone_denial(message: &str) -> bool {
    let normalized = message.to_lowercase();
    [
        "access denied",
        "permission denied",
        "privacy",
        "not authorized",
        "unauthorized",
        "denied by system",
        "device access is denied",
    ]
    .iter()
    .any(|needle| normalized.contains(needle))
}

#[cfg(target_os = "windows")]
fn open_microphone_settings() -> anyhow::Result<()> {
    std::process::Command::new("cmd")
        .args(["/C", "start", "", "ms-settings:privacy-microphone"])
        .spawn()
        .map(|_| ())
        .map_err(|error| anyhow!("failed to open Windows microphone settings: {error}"))
}

#[cfg(not(target_os = "windows"))]
fn open_microphone_settings() -> anyhow::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_requires_only_microphone() {
        let snapshot = microphone_snapshot(PermissionState::Granted);

        assert_eq!(snapshot.requirements.len(), 1);
        assert_eq!(snapshot.requirements[0].kind, PermissionKind::Microphone);
        assert!(snapshot.all_required_granted);
        assert_eq!(snapshot.microphone, Some(PermissionState::Granted));
        assert_eq!(snapshot.accessibility, None);
        assert_eq!(snapshot.input_monitoring, None);
    }

    #[test]
    fn denied_microphone_snapshot_is_not_ready() {
        let snapshot = microphone_snapshot(PermissionState::Denied);

        assert!(!snapshot.all_required_granted);
        assert_eq!(snapshot.requirements[0].state, PermissionState::Denied);
    }

    #[test]
    fn classifies_known_privacy_errors_as_denied() {
        assert!(is_known_microphone_denial(
            "Windows privacy settings returned access denied"
        ));
        assert!(is_known_microphone_denial("Permission denied"));
        assert!(!is_known_microphone_denial("No input devices were found"));
    }

    #[test]
    fn maps_mocked_microphone_probe_results() {
        assert_eq!(
            microphone_state_from_probe_result(Ok(())),
            PermissionState::Granted
        );
        assert_eq!(
            microphone_state_from_probe_result(Err(anyhow!(
                "Windows privacy settings returned access denied"
            ))),
            PermissionState::Denied
        );
        assert_eq!(
            microphone_state_from_probe_result(Err(anyhow!("No input devices were found"))),
            PermissionState::Unknown
        );
    }
}
