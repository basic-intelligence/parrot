import AppKit
import ApplicationServices
import AVFoundation
import CoreGraphics
import Foundation
import IOKit.hid

enum PermissionState: String, Codable, Sendable {
    case granted
    case denied
    case notDetermined
    case unknown
}

enum PermissionManager {
    static func snapshot() -> PermissionSnapshotDTO {
        let microphone = microphoneStatus()
        let accessibility = accessibilityStatus()
        let inputMonitoring = inputMonitoringStatus()
        let requirements = [
            PermissionRequirementDTO(
                kind: .microphone,
                title: "Microphone",
                description: "Record your voice locally for dictation.",
                state: microphone,
                required: true,
                requestable: true,
                opensSettings: microphone != .notDetermined
            ),
            PermissionRequirementDTO(
                kind: .accessibility,
                title: "Accessibility",
                description: "Consume the Parrot shortcut event and paste the finished text.",
                state: accessibility,
                required: true,
                requestable: true,
                opensSettings: true
            ),
            PermissionRequirementDTO(
                kind: .inputMonitoring,
                title: "Input Monitoring",
                description: "Some Macs require this so Parrot Core can listen for your shortcut while you use other apps.",
                state: inputMonitoring,
                required: false,
                requestable: true,
                opensSettings: true
            ),
        ]
        let allRequiredGranted = requirements
            .filter(\.required)
            .allSatisfy { $0.state == .granted }
        return PermissionSnapshotDTO(
            requirements: requirements,
            allRequiredGranted: allRequiredGranted,
            microphone: microphone,
            accessibility: accessibility,
            inputMonitoring: inputMonitoring,
            allGranted: microphone == .granted && accessibility == .granted
        )
    }

    static func microphoneStatus() -> PermissionState {
        switch AVCaptureDevice.authorizationStatus(for: .audio) {
        case .authorized:
            return .granted
        case .denied, .restricted:
            return .denied
        case .notDetermined:
            return .notDetermined
        @unknown default:
            return .unknown
        }
    }

    static func requestMicrophone(openSettings: Bool = false) async -> PermissionState {
        let current = microphoneStatus()

        if current == .notDetermined {
            let requested = await withCheckedContinuation { continuation in
                AVCaptureDevice.requestAccess(for: .audio) { granted in
                    continuation.resume(returning: granted ? .granted : microphoneStatus())
                }
            }

            if openSettings && requested != .granted {
                openPrivacyPane(anchor: "Privacy_Microphone")
            }

            return requested
        }

        if openSettings {
            openPrivacyPane(anchor: "Privacy_Microphone")
        }

        return current
    }

    static func accessibilityStatus() -> PermissionState {
        AXIsProcessTrusted() ? .granted : .denied
    }

    static func requestAccessibility(openSettings: Bool = false) -> PermissionState {
        if openSettings {
            openPrivacyPane(anchor: "Privacy_Accessibility")
            return accessibilityStatus()
        }

        if accessibilityStatus() == .granted {
            return .granted
        }

        // This shows the macOS Accessibility Access dialog.
        // That dialog already has an "Open System Settings" button,
        // so do not also call openPrivacyPane here.
        let promptKey = kAXTrustedCheckOptionPrompt.takeUnretainedValue() as String
        _ = AXIsProcessTrustedWithOptions([promptKey: true] as CFDictionary)

        return accessibilityStatus()
    }

    static func inputMonitoringStatus() -> PermissionState {
        if #available(macOS 10.15, *) {
            switch IOHIDCheckAccess(kIOHIDRequestTypeListenEvent) {
            case kIOHIDAccessTypeGranted:
                return .granted
            case kIOHIDAccessTypeDenied:
                return .denied
            case kIOHIDAccessTypeUnknown:
                return .notDetermined
            default:
                return .unknown
            }
        }

        return .unknown
    }

    static func requestInputMonitoring(openSettings: Bool = false) -> PermissionState {
        if openSettings {
            openPrivacyPane(anchor: "Privacy_ListenEvent")
            return inputMonitoringStatus()
        }

        if #available(macOS 10.15, *) {
            let granted = IOHIDRequestAccess(kIOHIDRequestTypeListenEvent)
            return granted ? .granted : inputMonitoringStatus()
        }

        return .unknown
    }

    static func openPrivacyPane(anchor: String) {
        let urls = [
            "x-apple.systempreferences:com.apple.preference.security?\(anchor)",
            "x-apple.systempreferences:com.apple.preference.security"
        ]

        for value in urls {
            guard let url = URL(string: value) else { continue }
            if NSWorkspace.shared.open(url) {
                return
            }
        }
    }
}
