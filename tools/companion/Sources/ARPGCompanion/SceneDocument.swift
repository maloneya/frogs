import Foundation

/// Opaque authored data travels intact. Swift never reconstructs a GameScene
/// from its displayed subset, so unedited gameplay relationships cannot vanish.
indirect enum JSONValue: Codable, Equatable, Sendable {
    case object([String: JSONValue]), array([JSONValue]), string(String), number(Double), unsigned(UInt64), signed(Int64), bool(Bool), null
    init(from decoder: Decoder) throws {
        let c = try decoder.singleValueContainer()
        if c.decodeNil() { self = .null }
        else if let v = try? c.decode(Bool.self) { self = .bool(v) }
        // Preserve authored integer values beyond Double's exact range.
        else if let v = try? c.decode(UInt64.self) { self = .unsigned(v) }
        else if let v = try? c.decode(Int64.self) { self = .signed(v) }
        else if let v = try? c.decode(Double.self) { self = .number(v) }
        else if let v = try? c.decode(String.self) { self = .string(v) }
        else if let v = try? c.decode([JSONValue].self) { self = .array(v) }
        else { self = .object(try c.decode([String: JSONValue].self)) }
    }
    func encode(to encoder: Encoder) throws {
        var c = encoder.singleValueContainer()
        switch self {
        case .null: try c.encodeNil()
        case .bool(let v): try c.encode(v)
        case .number(let v): try c.encode(v)
        case .unsigned(let v): try c.encode(v)
        case .signed(let v): try c.encode(v)
        case .string(let v): try c.encode(v)
        case .array(let v): try c.encode(v)
        case .object(let v): try c.encode(v)
        }
    }
    static func point(_ x: Double, _ z: Double) -> Self { .array([.number(x), .number(z)]) }
}

struct EditorRequest: Encodable, Sendable {
    let action: String
    var scene: JSONValue? = nil
    var edit: JSONValue? = nil
    var path: String? = nil
    var revision: String? = nil
    var play: Bool? = nil
}

struct SceneField: Decodable, Identifiable, Sendable {
    let key: String
    let label: String
    let unit: String
    let value: Double
    let step: Double
    let options: [String]?
    var id: String { key }
}

struct SceneObject: Decodable, Identifiable, Sendable {
    let id: String
    let kind: String
    let index: Int
    let label: String
    let x: Double
    let z: Double
    let width: Double
    let depth: Double
    let yaw: Double
    let radius: Double
    let fields: [SceneField]
    var target: JSONValue { .object(["kind": .string(kind), "index": .number(Double(index))]) }
    func value(_ field: String) -> Double { fields.first { $0.key == field }?.value ?? 0 }
}

struct SceneLandmark: Decodable, Sendable {
    let x: Double
    let z: Double
    let label: String
}

struct SceneDocument: Decodable, Sendable {
    let scene: JSONValue
    let name: String
    let objects: [SceneObject]
    let selected: String?
    let path: String?
    let revision: String?
    let arena_half: Double
    let player_radius: Double
    let trees: [SceneLandmark]
    let torches: [SceneLandmark]
    let relationships: Int
}

/// World X/Z metres and view pixels. The map uses a uniform scale even when
/// resized; snap rounds authored coordinates, never the simulation geometry.
struct SceneViewport {
    var centreX = 0.0
    var centreZ = 0.0
    var span = 48.0
    func scale(width: Double, height: Double) -> Double { min(width, height) / span }
    func world(x: Double, y: Double, width: Double, height: Double, snap: Double) -> (Double, Double) {
        let s = scale(width: width, height: height)
        let a = centreX + (x - width / 2) / s, b = centreZ - (y - height / 2) / s
        return snapped(x: a, z: b, spacing: snap)
    }
    func snapped(x: Double, z: Double, spacing: Double) -> (Double, Double) {
        if spacing > 0 { return ((x / spacing).rounded() * spacing, (z / spacing).rounded() * spacing) }
        return (x, z)
    }
    func screen(x: Double, z: Double, width: Double, height: Double) -> (Double, Double) {
        let s = scale(width: width, height: height)
        return (width / 2 + (x - centreX) * s, height / 2 - (z - centreZ) * s)
    }
    func hit(_ objects: [SceneObject], x: Double, z: Double, tolerance: Double) -> SceneObject? {
        objects.reversed().first { o in
            let dx = x - o.x, dz = z - o.z
            if o.kind == "wall" {
                let localX = dx * cos(o.yaw) - dz * sin(o.yaw)
                let localZ = dx * sin(o.yaw) + dz * cos(o.yaw)
                return abs(localX) <= o.width / 2 + tolerance && abs(localZ) <= o.depth / 2 + tolerance
            }
            if o.kind == "grid" {
                return dx >= -tolerance && dx <= o.width + tolerance && dz >= -tolerance && dz <= o.depth + tolerance
            }
            let distance = hypot(dx, dz)
            if o.kind == "source" { return distance <= tolerance || abs(distance - o.radius) <= tolerance }
            return distance <= max(o.radius, tolerance)
        }
    }
}
