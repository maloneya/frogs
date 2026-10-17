import Foundation
import Darwin

struct Attribute: Decodable, Identifiable, Sendable {
    let key: String
    let group: String
    let label: String
    let unit: String
    let `default`: Double
    let min: Double
    let max: Double
    let step: Double
    let value: Double
    let saved: Double
    let note: String?
    var id: String { key }
}

struct SceneSnapshot: Decodable, Sendable {
    let current: String?
    let run: UInt64
    let tick: UInt64
}

struct CaptureSnapshot: Decodable, Sendable {
    let pending: Bool
    let path: String?
    let error: String?
}

struct DebugSnapshot: Decodable, Sendable {
    let collision: Bool
    let vsync: Bool?
    let view_height: Double?
    let fps: Double
    let frame_ms: Double
    let enemies: UInt64
    let instances: UInt64
    let capture: CaptureSnapshot
    let performance: PerformanceSnapshot
}

struct PerformanceSnapshot: Decodable, Sendable {
    let frame_id: UInt64?
    let game_step_ms: Double?
    let presentation_ms: Double?
    let acquire_ms: Double?
    let upload_ms: Double?
    let encode_ms: Double?
    let submit_ms: Double?
    let present_ms: Double?
    let gpu_frame_id: UInt64?
    let gpu_span_ms: Double?
    let gpu_directional_shadow_ms: Double?
    let gpu_fire_shadow_ms: Double?
    let gpu_main_ms: Double?
}

struct Snapshot: Decodable, Sendable {
    let session: UInt32
    let fields: [Attribute]
    let unsaved: Bool
    let path: String
    let message: String
    let error: Bool
    let busy: Bool
    let scenes: SceneSnapshot
    let debug: DebugSnapshot
    var groups: [String] {
        fields.reduce(into: []) { if !$0.contains($1.group) { $0.append($1.group) } }
    }
}

struct Reply: Decodable, Sendable {
    let ok: Bool
    let error: String?
    let state: Snapshot?
}

enum Operation: String, Encodable, Sendable {
    case state, set, save, revert, defaults, restart, collision, vsync, zoom, capture
    case startScene = "start_scene"
}

enum ZoomDirection: String, Encodable, Sendable { case `in`, out }

struct Request: Encodable, Sendable {
    let op: Operation
    var session: UInt32? = nil
    var key: String? = nil
    var value: Double? = nil
    var path: String? = nil
    var enabled: Bool? = nil
    var direction: ZoomDirection? = nil
}

struct ConnectionError: LocalizedError {
    let message: String
    var errorDescription: String? { message }
}

/// The actor keeps blocking local I/O off the UI thread. Each exchange opens a
/// fresh socket, so reconnection needs no stale connection or replay queue.
actor Connection {
    let path: String
    init(path: String) { self.path = path }

    func exchange(_ request: Request) throws -> Reply {
        let fd = socket(AF_UNIX, SOCK_STREAM, 0)
        guard fd >= 0 else { throw failure() }
        defer { close(fd) }
        var timeout = timeval(tv_sec: 3, tv_usec: 0)
        var enabled: Int32 = 1
        guard setsockopt(fd, SOL_SOCKET, SO_RCVTIMEO, &timeout, socklen_t(MemoryLayout.size(ofValue: timeout))) == 0,
              setsockopt(fd, SOL_SOCKET, SO_SNDTIMEO, &timeout, socklen_t(MemoryLayout.size(ofValue: timeout))) == 0,
              setsockopt(fd, SOL_SOCKET, SO_NOSIGPIPE, &enabled, socklen_t(MemoryLayout.size(ofValue: enabled))) == 0 else { throw failure() }
        var address = sockaddr_un()
        address.sun_family = sa_family_t(AF_UNIX)
        address.sun_len = UInt8(MemoryLayout<sockaddr_un>.size)
        let bytes = Array(path.utf8) + [0]
        guard bytes.count <= MemoryLayout.size(ofValue: address.sun_path) else {
            throw ConnectionError(message: "Socket path is too long")
        }
        withUnsafeMutableBytes(of: &address.sun_path) { $0.copyBytes(from: bytes) }
        let connected = withUnsafePointer(to: &address) {
            $0.withMemoryRebound(to: sockaddr.self, capacity: 1) {
                Darwin.connect(fd, $0, socklen_t(MemoryLayout<sockaddr_un>.size))
            }
        }
        guard connected == 0 else { throw failure() }
        var payload = try JSONEncoder().encode(request)
        payload.append(10)
        try payload.withUnsafeBytes { buffer in
            var offset = 0
            while offset < buffer.count {
                let count = Darwin.write(fd, buffer.baseAddress!.advanced(by: offset), buffer.count - offset)
                if count < 0 && errno == EINTR { continue }
                guard count > 0 else { throw failure() }
                offset += count
            }
        }
        var response = Data()
        var buffer = [UInt8](repeating: 0, count: 4096)
        while response.count <= 8 * 1024 * 1024 {
            let count = Darwin.read(fd, &buffer, buffer.count)
            if count < 0 && errno == EINTR { continue }
            guard count > 0 else { throw ConnectionError(message: "Game disconnected or did not reply. Refresh before retrying an edit.") }
            response.append(contentsOf: buffer.prefix(count))
            if response.last == 10 { return try JSONDecoder().decode(Reply.self, from: response) }
        }
        throw ConnectionError(message: "Game reply is too large")
    }

    private func failure() -> ConnectionError {
        ConnectionError(message: String(cString: strerror(errno)))
    }
}
