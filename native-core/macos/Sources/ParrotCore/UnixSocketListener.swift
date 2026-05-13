import Darwin
import Foundation

final class UnixSocketListener {
    private var fd: Int32
    private let path: String

    init(path: String) throws {
        self.path = path
        fd = socket(AF_UNIX, SOCK_STREAM, 0)
        guard fd >= 0 else {
            throw ModelPipelineError.transcriptionFailed(String(cString: strerror(errno)))
        }

        try? FileManager.default.removeItem(atPath: path)

        var address = sockaddr_un()
        address.sun_family = sa_family_t(AF_UNIX)

        let pathBytes = Array(path.utf8)
        let sunPathSize = MemoryLayout.size(ofValue: address.sun_path)
        guard pathBytes.count < sunPathSize else {
            closeListener()
            throw ModelPipelineError.transcriptionFailed("parrot-whisper socket path is too long.")
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

        let bindResult = withUnsafePointer(to: &address) { pointer in
            pointer.withMemoryRebound(to: sockaddr.self, capacity: 1) { socketAddress in
                Darwin.bind(fd, socketAddress, socklen_t(MemoryLayout<sockaddr_un>.size))
            }
        }

        guard bindResult == 0 else {
            let message = String(cString: strerror(errno))
            closeListener()
            throw ModelPipelineError.transcriptionFailed("Could not bind parrot-whisper socket: \(message)")
        }

        guard Darwin.listen(fd, 1) == 0 else {
            let message = String(cString: strerror(errno))
            closeListener()
            throw ModelPipelineError.transcriptionFailed("Could not listen on parrot-whisper socket: \(message)")
        }
    }

    deinit {
        closeListener()
        try? FileManager.default.removeItem(atPath: path)
    }

    private func closeListener() {
        guard fd >= 0 else { return }
        close(fd)
        fd = -1
    }

    func accept(timeoutSeconds: Int) throws -> FileHandle {
        var pollFD = pollfd(fd: fd, events: Int16(POLLIN), revents: 0)

        while true {
            pollFD.revents = 0
            let pollResult = Darwin.poll(&pollFD, 1, Int32(timeoutSeconds * 1000))
            if pollResult > 0 {
                break
            }

            if pollResult == 0 {
                throw ModelPipelineError.transcriptionFailed("Timed out waiting for parrot-whisper helper.")
            }

            if errno == EINTR {
                continue
            }

            throw ModelPipelineError.transcriptionFailed(String(cString: strerror(errno)))
        }

        let clientFD = Darwin.accept(fd, nil, nil)
        guard clientFD >= 0 else {
            throw ModelPipelineError.transcriptionFailed(String(cString: strerror(errno)))
        }

        return FileHandle(fileDescriptor: clientFD, closeOnDealloc: true)
    }
}
