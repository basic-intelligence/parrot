import Darwin
import Foundation
import WhisperCppBridge

private struct CLIOptions {
    var modelPath: String?
    var audioPath: String?
    var socketPath: String?
    var languageCode: String?
    var detectLanguage = false
    var validateOnly = false
}

private struct WhisperHelperRequest: Codable {
    let id: String
    let audioPath: String
    let languageCode: String?
    let detectLanguage: Bool
}

private struct WhisperHelperResponse: Codable {
    let id: String
    let ok: Bool
    let result: WhisperCppTranscriptionResult?
    let error: String?
}

private enum CLIError: Error, LocalizedError {
    case missingValue(String)
    case unknownArgument(String)
    case missingModelPath
    case missingAudioPath
    case invalidAudioData

    var errorDescription: String? {
        switch self {
        case .missingValue(let flag):
            return "\(flag) requires a value."
        case .unknownArgument(let argument):
            return "Unknown argument: \(argument)"
        case .missingModelPath:
            return "--model is required."
        case .missingAudioPath:
            return "--audio is required unless --validate or --socket is set."
        case .invalidAudioData:
            return "Audio data must be raw Float32 samples."
        }
    }
}

private enum SocketConnectionError: Error, LocalizedError {
    case pathTooLong(String)
    case socketCreationFailed(String)
    case connectFailed(String)
    case readFailed(String)

    var errorDescription: String? {
        switch self {
        case .pathTooLong(let path):
            return "parrot-whisper socket path is too long: \(path)"
        case .socketCreationFailed(let message):
            return "Could not create parrot-whisper socket: \(message)"
        case .connectFailed(let message):
            return "Could not connect parrot-whisper socket: \(message)"
        case .readFailed(let message):
            return "Could not read parrot-whisper socket: \(message)"
        }
    }
}

private func parseArguments(_ arguments: [String]) throws -> CLIOptions {
    var options = CLIOptions()
    var index = 0

    func value(after flag: String) throws -> String {
        let valueIndex = index + 1
        guard valueIndex < arguments.count else { throw CLIError.missingValue(flag) }
        index = valueIndex
        return arguments[valueIndex]
    }

    while index < arguments.count {
        let argument = arguments[index]
        switch argument {
        case "--model":
            options.modelPath = try value(after: argument)
        case "--audio":
            options.audioPath = try value(after: argument)
        case "--socket":
            options.socketPath = try value(after: argument)
        case "--language":
            options.languageCode = try value(after: argument)
        case "--detect-language":
            options.detectLanguage = true
        case "--validate":
            options.validateOnly = true
        default:
            throw CLIError.unknownArgument(argument)
        }
        index += 1
    }

    return options
}

private func loadFloatSamples(from url: URL) throws -> [Float] {
    let data = try Data(contentsOf: url)
    guard data.count % MemoryLayout<Float>.stride == 0 else {
        throw CLIError.invalidAudioData
    }

    var samples = [Float](repeating: 0, count: data.count / MemoryLayout<Float>.stride)
    _ = samples.withUnsafeMutableBytes { buffer in
        data.copyBytes(to: buffer)
    }
    return samples
}

private func connectUnixSocket(path: String) throws -> FileHandle {
    let fd = socket(AF_UNIX, SOCK_STREAM, 0)
    guard fd >= 0 else {
        throw SocketConnectionError.socketCreationFailed(String(cString: strerror(errno)))
    }

    var address = sockaddr_un()
    address.sun_family = sa_family_t(AF_UNIX)

    let pathBytes = Array(path.utf8)
    let sunPathSize = MemoryLayout.size(ofValue: address.sun_path)
    guard pathBytes.count < sunPathSize else {
        close(fd)
        throw SocketConnectionError.pathTooLong(path)
    }

    withUnsafeMutablePointer(to: &address.sun_path) { pointer in
        pointer.withMemoryRebound(to: CChar.self, capacity: sunPathSize) { buffer in
            for index in 0..<sunPathSize {
                buffer[index] = 0
            }
            for (index, byte) in pathBytes.enumerated() {
                buffer[index] = CChar(bitPattern: byte)
            }
        }
    }

    let result = withUnsafePointer(to: &address) { pointer in
        pointer.withMemoryRebound(to: sockaddr.self, capacity: 1) { socketAddress in
            Darwin.connect(fd, socketAddress, socklen_t(MemoryLayout<sockaddr_un>.size))
        }
    }

    guard result == 0 else {
        let message = String(cString: strerror(errno))
        close(fd)
        throw SocketConnectionError.connectFailed(message)
    }

    return FileHandle(fileDescriptor: fd, closeOnDealloc: true)
}

private final class JSONLineReader {
    private let handle: FileHandle
    private var buffer = Data()

    init(handle: FileHandle) {
        self.handle = handle
    }

    func readLine() throws -> String? {
        while true {
            if let newlineIndex = buffer.firstIndex(of: 0x0A) {
                let lineData = buffer.prefix(upTo: newlineIndex)
                buffer.removeSubrange(buffer.startIndex...newlineIndex)
                return String(data: lineData, encoding: .utf8)
            }

            let chunk = try readChunk(upToCount: 4096)
            if chunk.isEmpty {
                guard buffer.isEmpty == false else {
                    return nil
                }

                let line = String(data: buffer, encoding: .utf8)
                buffer.removeAll()
                return line
            }

            buffer.append(chunk)
        }
    }

    private func readChunk(upToCount count: Int) throws -> Data {
        var bytes = [UInt8](repeating: 0, count: count)

        while true {
            let readCount = bytes.withUnsafeMutableBytes { buffer -> Int in
                guard let baseAddress = buffer.baseAddress else {
                    return 0
                }
                return Darwin.read(handle.fileDescriptor, baseAddress, count)
            }

            if readCount > 0 {
                return Data(bytes.prefix(readCount))
            }

            if readCount == 0 {
                return Data()
            }

            if errno == EINTR {
                continue
            }

            throw SocketConnectionError.readFailed(String(cString: strerror(errno)))
        }
    }
}

private func writeJSONLine<T: Encodable>(_ value: T, to handle: FileHandle) throws {
    let data = try JSONEncoder().encode(value)
    handle.write(data)
    handle.write(Data([0x0A]))
}

private func runPersistent(model: WhisperCppSpeechModel, socketPath: String) throws {
    let socket = try connectUnixSocket(path: socketPath)
    let reader = JSONLineReader(handle: socket)
    let decoder = JSONDecoder()

    while let line = try reader.readLine() {
        guard let data = line.data(using: .utf8),
              !line.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
        else { continue }

        let request: WhisperHelperRequest
        do {
            request = try decoder.decode(WhisperHelperRequest.self, from: data)
        } catch {
            try writeJSONLine(
                WhisperHelperResponse(
                    id: "unknown",
                    ok: false,
                    result: nil,
                    error: error.localizedDescription
                ),
                to: socket
            )
            continue
        }

        do {
            let samples = try loadFloatSamples(from: URL(fileURLWithPath: request.audioPath))
            let result = try model.transcribe(
                samples: samples,
                languageCode: request.languageCode,
                detectLanguage: request.detectLanguage
            )
            try writeJSONLine(
                WhisperHelperResponse(
                    id: request.id,
                    ok: true,
                    result: result,
                    error: nil
                ),
                to: socket
            )
        } catch {
            try writeJSONLine(
                WhisperHelperResponse(
                    id: request.id,
                    ok: false,
                    result: nil,
                    error: error.localizedDescription
                ),
                to: socket
            )
        }
    }
}

private func run() throws {
    let options = try parseArguments(Array(CommandLine.arguments.dropFirst()))
    guard let modelPath = options.modelPath else { throw CLIError.missingModelPath }

    let model = try WhisperCppSpeechModel(modelURL: URL(fileURLWithPath: modelPath))
    if options.validateOnly { return }

    if let socketPath = options.socketPath {
        try runPersistent(model: model, socketPath: socketPath)
        return
    }

    guard let audioPath = options.audioPath else { throw CLIError.missingAudioPath }
    let samples = try loadFloatSamples(from: URL(fileURLWithPath: audioPath))
    let result = try model.transcribe(
        samples: samples,
        languageCode: options.languageCode,
        detectLanguage: options.detectLanguage
    )

    let data = try JSONEncoder().encode(result)
    FileHandle.standardOutput.write(data)
    FileHandle.standardOutput.write(Data([0x0A]))
}

do {
    try run()
} catch {
    fputs("\(error.localizedDescription)\n", stderr)
    exit(1)
}
