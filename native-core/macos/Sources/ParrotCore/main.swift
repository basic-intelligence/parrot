import Darwin
import Foundation

let service = CoreService()
let decoder = JSONDecoder.parrot

func emit(_ response: JSONResponse) {
    JSONLineWriter.shared.write(response)
}

func socketPathArgument() -> String? {
    let args = CommandLine.arguments
    guard let index = args.firstIndex(of: "--socket"), args.indices.contains(index + 1) else {
        return nil
    }
    return args[index + 1]
}

enum SocketConnectionError: Error, LocalizedError {
    case pathTooLong(String)
    case socketCreationFailed(String)
    case connectFailed(String)
    case readFailed(String)

    var errorDescription: String? {
        switch self {
        case .pathTooLong(let path):
            return "Native core socket path is too long: \(path)"
        case .socketCreationFailed(let message):
            return "Could not create native core socket: \(message)"
        case .connectFailed(let message):
            return "Could not connect native core socket: \(message)"
        case .readFailed(let message):
            return "Could not read native core socket: \(message)"
        }
    }
}

func connectUnixSocket(path: String) throws -> FileHandle {
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

final class JSONLineReader {
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

func handleLine(_ line: String) async {
    guard let data = line.data(using: .utf8),
          !line.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
        return
    }

    do {
        let request = try decoder.decode(JSONRequest.self, from: data)
        let payload = try await service.handle(request)
        emit(JSONResponse(id: request.id, ok: true, payload: payload, error: nil))
    } catch {
        let id = (try? decoder.decode(JSONRequest.self, from: data).id) ?? "unknown"
        emit(JSONResponse(id: id, ok: false, payload: nil, error: error.localizedDescription))
    }
}

do {
    if let socketPath = socketPathArgument() {
        let socket = try connectUnixSocket(path: socketPath)
        JSONLineWriter.shared.configure(output: socket)

        let reader = JSONLineReader(handle: socket)
        while let line = try reader.readLine() {
            await handleLine(line)
        }
        await service.shutdown()
    } else {
        while let line = readLine() {
            await handleLine(line)
        }
        await service.shutdown()
    }
} catch {
    FileHandle.standardError.write(Data("ParrotCore failed to start: \(error.localizedDescription)\n".utf8))
    exit(1)
}
