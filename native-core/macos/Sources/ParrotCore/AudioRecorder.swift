import AppKit
import AVFoundation
import CoreAudio
import Foundation

final class AudioRecorder {
    var preferredInputUID: String?

    private var engine = AVAudioEngine()
    private var configuredDeviceID: AudioDeviceID?
    private let bufferLock = NSLock()
    private var samples: [Float] = []

    private lazy var targetFormat = AVAudioFormat(commonFormat: .pcmFormatFloat32, sampleRate: 16_000, channels: 1, interleaved: false)!
    private var cachedConverter: AVAudioConverter?
    private var cachedConverterSourceFormat: AVAudioFormat?
    private var wakeObserver: NSObjectProtocol?

    init() {
        InputDeviceManager.observeDeviceChanges { [weak self] in
            self?.debugLog("CoreAudio device/default-input changed; rebuilding engine")
            self?.resetForRouteChange()
        }
        wakeObserver = NSWorkspace.shared.notificationCenter.addObserver(
            forName: NSWorkspace.didWakeNotification,
            object: nil,
            queue: nil
        ) { [weak self] _ in
            self?.debugLog("system wake observed; rebuilding engine")
            self?.resetForRouteChange()
        }
    }

    deinit {
        if let wakeObserver {
            NSWorkspace.shared.notificationCenter.removeObserver(wakeObserver)
        }
    }

    func start() throws {
        resetBuffer()
        engine.stop()
        engine.inputNode.removeTap(onBus: 0)
        rebuildIfNeededForCurrentDevice()

        do {
            try installTapAndStart()
        } catch {
            debugLog("first start failed: \(error.localizedDescription); rebuilding and retrying once")
            rebuildEngine()
            rebuildIfNeededForCurrentDevice(force: true)
            try installTapAndStart()
        }
    }

    func stop() async -> [Float] {
        try? await Task.sleep(nanoseconds: 25_000_000)
        engine.inputNode.removeTap(onBus: 0)
        engine.stop()
        return snapshot()
    }

    func resetForRouteChange() {
        rebuildEngine()
    }

    private func installTapAndStart() throws {
        let input = engine.inputNode
        let hwFormat = input.inputFormat(forBus: 0)
        debugLog("hardware input format sampleRate=\(hwFormat.sampleRate) channels=\(hwFormat.channelCount)")
        guard hwFormat.sampleRate > 0, hwFormat.channelCount > 0 else {
            throw RecorderError.noUsableInputDevice
        }

        cachedConverter = nil
        cachedConverterSourceFormat = nil
        let bufferSize = max(1, AVAudioFrameCount(hwFormat.sampleRate * 0.02))
        input.installTap(onBus: 0, bufferSize: bufferSize, format: hwFormat) { [weak self] buffer, _ in
            self?.convertAndAppend(buffer)
        }
        try engine.start()
    }

    private func rebuildIfNeededForCurrentDevice(force: Bool = false) {
        let resolution = InputDeviceManager.resolveInputDevice(preferredUID: preferredInputUID)
        let resolvedID = resolution.device?.id
        debugLog("selectedUID=\(preferredInputUID ?? "system-default") resolvedDeviceID=\(resolvedID.map(String.init) ?? "nil")")
        if resolution.usedFallback {
            debugLog("selected input UID unavailable; falling back to system default")
        }

        guard force || resolvedID != configuredDeviceID else { return }

        if configuredDeviceID != nil || force {
            rebuildEngine()
        }

        guard let resolvedID else { return }
        guard let audioUnit = engine.inputNode.audioUnit else {
            configuredDeviceID = nil
            debugLog("inputNode.audioUnit is unavailable; using system default input")
            return
        }

        var deviceID = resolvedID
        let status = AudioUnitSetProperty(
            audioUnit,
            kAudioOutputUnitProperty_CurrentDevice,
            kAudioUnitScope_Global,
            0,
            &deviceID,
            UInt32(MemoryLayout<AudioDeviceID>.size)
        )

        if status == noErr {
            configuredDeviceID = resolvedID
            debugLog("configured CoreAudio input deviceID=\(resolvedID)")
        } else {
            configuredDeviceID = nil
            debugLog("failed to set CoreAudio input deviceID=\(resolvedID), status=\(status)")
        }
    }

    private func rebuildEngine() {
        engine.stop()
        engine.inputNode.removeTap(onBus: 0)
        engine = AVAudioEngine()
        configuredDeviceID = nil
        cachedConverter = nil
        cachedConverterSourceFormat = nil
    }

    private func converter(for sourceFormat: AVAudioFormat) -> AVAudioConverter? {
        if let cachedConverter, let cachedConverterSourceFormat,
           cachedConverterSourceFormat.sampleRate == sourceFormat.sampleRate,
           cachedConverterSourceFormat.channelCount == sourceFormat.channelCount {
            return cachedConverter
        }
        let converter = AVAudioConverter(from: sourceFormat, to: targetFormat)
        cachedConverter = converter
        cachedConverterSourceFormat = sourceFormat
        return converter
    }

    private func convertAndAppend(_ buffer: AVAudioPCMBuffer) {
        if buffer.format.channelCount > 2 {
            convertWithManualDownmix(buffer)
            return
        }
        guard let converter = converter(for: buffer.format) else { return }
        convert(buffer: buffer, using: converter)
    }

    private func convert(buffer: AVAudioPCMBuffer, using converter: AVAudioConverter) {
        let capacity = AVAudioFrameCount(Double(buffer.frameLength) * targetFormat.sampleRate / buffer.format.sampleRate) + 1
        guard let out = AVAudioPCMBuffer(pcmFormat: targetFormat, frameCapacity: capacity) else { return }
        var consumed = false
        var error: NSError?
        converter.convert(to: out, error: &error) { _, status in
            if consumed { status.pointee = .noDataNow; return nil }
            consumed = true
            status.pointee = .haveData
            return buffer
        }
        guard error == nil, let channel = out.floatChannelData, out.frameLength > 0 else { return }
        append(Array(UnsafeBufferPointer(start: channel[0], count: Int(out.frameLength))))
    }

    private func convertWithManualDownmix(_ buffer: AVAudioPCMBuffer) {
        guard let data = buffer.floatChannelData else { return }
        let channels = Int(buffer.format.channelCount)
        let frames = Int(buffer.frameLength)
        guard channels > 0, frames > 0 else { return }
        var mono = [Float](repeating: 0, count: frames)
        for frame in 0..<frames {
            var sum: Float = 0
            for channel in 0..<channels { sum += data[channel][frame] }
            mono[frame] = sum / Float(channels)
        }
        let monoFormat = AVAudioFormat(commonFormat: .pcmFormatFloat32, sampleRate: buffer.format.sampleRate, channels: 1, interleaved: false)!
        guard let monoBuffer = AVAudioPCMBuffer(pcmFormat: monoFormat, frameCapacity: AVAudioFrameCount(frames)) else { return }
        monoBuffer.frameLength = AVAudioFrameCount(frames)
        mono.withUnsafeBufferPointer { source in
            monoBuffer.floatChannelData![0].update(from: source.baseAddress!, count: frames)
        }
        guard let converter = converter(for: monoFormat) else { return }
        convert(buffer: monoBuffer, using: converter)
    }

    private func append(_ newSamples: [Float]) {
        bufferLock.lock()
        samples.append(contentsOf: newSamples)
        bufferLock.unlock()
    }

    private func resetBuffer() {
        bufferLock.lock()
        samples = []
        bufferLock.unlock()
    }

    private func snapshot() -> [Float] {
        bufferLock.lock()
        defer { bufferLock.unlock() }
        return samples
    }

    private func debugLog(_ message: String) {
        fputs("ParrotCore AudioRecorder: \(message)\n", stderr)
    }
}

enum RecorderError: Error, LocalizedError {
    case noUsableInputDevice
    var errorDescription: String? { "No usable input device is available." }
}
