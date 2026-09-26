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
    let colour: [Double]?
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

struct SceneDocument: Decodable, Sendable {
    let scene: JSONValue
    let name: String
    let objects: [SceneObject]
    let selected: String?
    let path: String?
    let revision: String?
    let arena_half: Double
    let player_radius: Double
    let projection: SceneProjection
    let lighting: [SceneField]
    let relationships: Int
}

/// The camera supplies this ground-plane matrix; Swift owns no camera angles.
struct SceneProjection: Decodable, Sendable {
    let right: [Double]
    let up: [Double]
    init(_ rows: [[Double]]) {
        precondition(Self.valid(rows))
        right = rows[0]; up = rows[1]
    }
    init(from decoder: Decoder) throws {
        let container = try decoder.singleValueContainer()
        let rows = try container.decode([[Double]].self)
        guard Self.valid(rows) else {
            throw DecodingError.dataCorruptedError(in: container, debugDescription: "Invalid scene camera projection")
        }
        right = rows[0]; up = rows[1]
    }
    private static func valid(_ rows: [[Double]]) -> Bool {
        rows.count == 2 && rows.allSatisfy { $0.count == 2 && $0.allSatisfy(\.isFinite) }
            && abs(rows[0][0] * rows[1][1] - rows[0][1] * rows[1][0]) > 0.000001
    }
    func project(x: Double, z: Double) -> (Double, Double) {
        (right[0] * x + right[1] * z, up[0] * x + up[1] * z)
    }
    /// Conservative world-space pick margin for a screen-plane radius.
    /// The inverse matrix norm keeps compressed ground footprints selectable.
    func worldTolerance(_ radius: Double) -> Double {
        let (a,b) = unproject(right: radius, up: 0)
        let (c,d) = unproject(right: 0, up: radius)
        return sqrt(a*a + b*b + c*c + d*d)
    }
    func unproject(right x: Double, up y: Double) -> (Double, Double) {
        let determinant = right[0] * up[1] - right[1] * up[0]
        return ((up[1] * x - right[1] * y) / determinant, (-up[0] * x + right[0] * y) / determinant)
    }
}

/// Camera-plane metres and view pixels. Snapping always rounds world X/Z,
/// after inverse projection, so camera alignment cannot alter authored geometry.
struct SceneViewport {
    var centreX = 0.0
    var centreZ = 0.0
    var span = 48.0
    func scale(width: Double, height: Double) -> Double { min(width, height) / span }
    /// At most roughly 80 world grid lines per axis, including extreme light
    /// ranges and wide windows. Use readable 1/2/5 metre increments.
    static func gridSpacing(extent: Double) -> Double {
        let target = max(1, extent / 80)
        let magnitude = pow(10, floor(log10(target)))
        let fraction = target / magnitude
        return magnitude * (fraction <= 1 ? 1 : fraction <= 2 ? 2 : fraction <= 5 ? 5 : 10)
    }
    func world(x: Double, y: Double, width: Double, height: Double, snap: Double, projection: SceneProjection) -> (Double, Double) {
        let (dx, dz) = delta(x: x - width / 2, y: y - height / 2, width: width, height: height, projection: projection)
        return snapped(x: centreX + dx, z: centreZ + dz, spacing: snap)
    }
    func delta(x: Double, y: Double, width: Double, height: Double, projection: SceneProjection) -> (Double, Double) {
        let s = scale(width: width, height: height)
        return projection.unproject(right: x / s, up: -y / s)
    }
    func snapped(x: Double, z: Double, spacing: Double) -> (Double, Double) {
        if spacing > 0 { return ((x / spacing).rounded() * spacing, (z / spacing).rounded() * spacing) }
        return (x, z)
    }
    func screen(x: Double, z: Double, width: Double, height: Double, projection: SceneProjection) -> (Double, Double) {
        let s = scale(width: width, height: height)
        let (right, up) = projection.project(x: x - centreX, z: z - centreZ)
        return (width / 2 + right * s, height / 2 - up * s)
    }
    mutating func fit(_ document: SceneDocument) {
        var points: [(Double, Double)] = [(0, 0)]
        for o in document.objects {
            // Conservative world bounds include rotated walls and source ranges.
            let extent = max(o.width, o.depth) + o.radius
            for dx in [-extent, extent] { for dz in [-extent, extent] {
                points.append(document.projection.project(x: o.x + dx, z: o.z + dz))
            } }
        }
        let xs = points.map { $0.0 }, ys = points.map { $0.1 }
        let (x, z) = document.projection.unproject(right: (xs.min()! + xs.max()!) / 2, up: (ys.min()! + ys.max()!) / 2)
        centreX = x; centreZ = z
        span = max(16, max(xs.max()! - xs.min()!, ys.max()! - ys.min()!) * 1.15)
    }
    func hit(_ objects: [SceneObject], x: Double, z: Double, tolerance: Double) -> SceneObject? {
        objects.reversed().first { o in
            let dx = x - o.x, dz = z - o.z
            if o.kind == "wall" || o.kind == "tree" {
                let localX = dx * cos(o.yaw) - dz * sin(o.yaw)
                let localZ = dx * sin(o.yaw) + dz * cos(o.yaw)
                return abs(localX) <= o.width / 2 + tolerance && abs(localZ) <= o.depth / 2 + tolerance
            }
            if o.kind == "grid" {
                return dx >= -tolerance && dx <= o.width + tolerance && dz >= -tolerance && dz <= o.depth + tolerance
            }
            let distance = hypot(dx, dz)
            if o.kind == "source" || o.kind == "campfire" { return distance <= tolerance || abs(distance - o.radius) <= tolerance }
            return distance <= max(o.radius, tolerance)
        }
    }
}
