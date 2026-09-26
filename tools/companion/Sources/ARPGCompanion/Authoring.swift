import Foundation

struct AuthoringReply: Decodable, Sendable {
    let ok: Bool
    let error: String?
    let editor: SceneDocument?
}

/// Runs the same headless executable used by agents, off the UI thread.
/// Each process consumes one complete request; no session or game is involved.
actor Authoring {
    let executable: URL
    init() {
        if let path = ProcessInfo.processInfo.environment["ARPG_AUTHORING_BIN"] {
            executable = URL(fileURLWithPath: path)
        } else {
            executable = Bundle.main.executableURL!.deletingLastPathComponent().appendingPathComponent("arpg-author")
        }
    }
    func exchange(_ request: EditorRequest) throws -> AuthoringReply {
        let process = Process(), input = Pipe(), output = Pipe()
        process.executableURL = executable
        process.standardInput = input; process.standardOutput = output
        process.standardError = FileHandle.standardError
        let payload = try JSONEncoder().encode(request)
        guard payload.count <= 8 * 1024 * 1024 else { throw ConnectionError(message: "Scene request exceeds 8 MiB.") }
        try process.run()
        do {
            try input.fileHandleForWriting.write(contentsOf: payload)
            try input.fileHandleForWriting.close()
            var response = Data()
            while let chunk = try output.fileHandleForReading.read(upToCount: 64 * 1024), !chunk.isEmpty {
                guard response.count + chunk.count <= 8 * 1024 * 1024 else { throw ConnectionError(message: "Authoring reply exceeds 8 MiB.") }
                response.append(chunk)
            }
            process.waitUntilExit()
            return try JSONDecoder().decode(AuthoringReply.self, from: response)
        } catch {
            try? input.fileHandleForWriting.close()
            if process.isRunning { process.terminate() }
            process.waitUntilExit()
            throw error
        }
    }
}
