import Foundation

final class JSONLineWriter {
    static let shared = JSONLineWriter()

    private let lock = NSLock()
    private let encoder = JSONEncoder.parrot
    private var output = FileHandle.standardOutput

    func configure(output: FileHandle) {
        lock.lock()
        self.output = output
        lock.unlock()
    }

    func write<T: Encodable>(_ value: T) {
        lock.lock()
        defer { lock.unlock() }

        do {
            let data = try encoder.encode(value)
            output.write(data)
            output.write(Data("\n".utf8))
        } catch {
            FileHandle.standardError.write(Data("failed to encode json line: \(error)\n".utf8))
        }
    }

    func event(_ name: String, payload: JSONValue) {
        write(JSONEvent(event: name, payload: payload))
    }
}
