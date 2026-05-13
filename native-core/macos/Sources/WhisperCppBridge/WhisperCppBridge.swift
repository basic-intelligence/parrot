import Foundation
import whisper

public struct WhisperCppTranscriptionResult: Codable, Sendable {
    public let text: String
    public let languageCode: String?

    public init(text: String, languageCode: String?) {
        self.text = text
        self.languageCode = languageCode
    }
}

public enum WhisperCppBridgeError: Error, LocalizedError {
    case modelLoadFailed(String)
    case transcriptionFailed(String)

    public var errorDescription: String? {
        switch self {
        case .modelLoadFailed(let message):
            return "Could not load the whisper.cpp speech model. \(message)"
        case .transcriptionFailed(let message):
            return "whisper.cpp transcription failed. \(message)"
        }
    }
}

public final class WhisperCppSpeechModel {
    private let context: OpaquePointer

    public init(modelURL: URL) throws {
        var params = whisper_context_default_params()
        params.use_gpu = false

        guard let loadedContext = modelURL.path.withCString({ path in
            whisper_init_from_file_with_params(path, params)
        }) else {
            throw WhisperCppBridgeError.modelLoadFailed(
                "whisper.cpp could not load \(modelURL.lastPathComponent)."
            )
        }

        context = loadedContext
    }

    deinit {
        whisper_free(context)
    }

    public func transcribe(
        samples: [Float],
        languageCode: String?,
        detectLanguage: Bool
    ) throws -> WhisperCppTranscriptionResult {
        var params = whisper_full_default_params(WHISPER_SAMPLING_GREEDY)
        params.n_threads = Int32(ProcessInfo.processInfo.processorCount)
        params.no_context = true
        params.no_timestamps = true
        params.print_progress = false
        params.print_realtime = false
        params.print_timestamps = false
        params.translate = false
        params.detect_language = detectLanguage

        let requestedLanguage = detectLanguage ? "auto" : languageCode
        return try withOptionalCString(requestedLanguage) { languagePointer in
            params.language = languagePointer

            let status = samples.withUnsafeBufferPointer { buffer -> Int32 in
                guard let baseAddress = buffer.baseAddress else { return -1 }
                return whisper_full(context, params, baseAddress, Int32(buffer.count))
            }
            guard status == 0 else {
                throw WhisperCppBridgeError.transcriptionFailed("Status \(status).")
            }

            var segments: [String] = []
            let segmentCount = whisper_full_n_segments(context)
            if segmentCount > 0 {
                for index in 0..<Int(segmentCount) {
                    if let segment = whisper_full_get_segment_text(context, Int32(index)) {
                        segments.append(String(cString: segment))
                    }
                }
            }

            let detectedLanguageCode: String?
            if detectLanguage {
                let languageID = whisper_full_lang_id(context)
                if languageID >= 0, let language = whisper_lang_str(languageID) {
                    detectedLanguageCode = String(cString: language)
                } else {
                    detectedLanguageCode = nil
                }
            } else {
                detectedLanguageCode = languageCode
            }

            return WhisperCppTranscriptionResult(
                text: segments.joined(separator: " ").trimmingCharacters(in: .whitespacesAndNewlines),
                languageCode: detectedLanguageCode
            )
        }
    }

    private func withOptionalCString<T>(
        _ value: String?,
        _ body: (UnsafePointer<CChar>?) throws -> T
    ) rethrows -> T {
        guard let value else {
            return try body(nil)
        }
        return try value.withCString { pointer in
            try body(pointer)
        }
    }
}
