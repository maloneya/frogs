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
    var document: JSONValue? = nil
    var edit: JSONValue? = nil
    var path: String? = nil
    var revision: String? = nil
}

struct SceneField: Decodable, Identifiable, Sendable {
    let key: String
    let label: String
    let unit: String
    private let rawValue: JSONValue
    let step: Double
    let options: [String]?
    let colour: [Double]?
    let integer: Bool
    var id: String { key }
    var integerValue: UInt64? {
        guard integer, case .unsigned(let n) = rawValue else { return nil }
        return n
    }
    var value: Double {
        switch rawValue {
        case .unsigned(let n): return Double(n)
        case .signed(let n): return Double(n)
        case .number(let n): return n
        default: preconditionFailure("Scene fields must be numeric")
        }
    }
    private enum CodingKeys: String, CodingKey { case key, label, unit, value, step, options, colour, integer }
    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        key = try c.decode(String.self, forKey: .key)
        label = try c.decode(String.self, forKey: .label)
        unit = try c.decode(String.self, forKey: .unit)
        integer = try c.decodeIfPresent(Bool.self, forKey: .integer) ?? false
        if integer {
            rawValue = .unsigned(try c.decode(UInt64.self, forKey: .value))
        } else {
            rawValue = .number(try c.decode(Double.self, forKey: .value))
        }
        step = try c.decode(Double.self, forKey: .step)
        options = try c.decodeIfPresent([String].self, forKey: .options)
        colour = try c.decodeIfPresent([Double].self, forKey: .colour)
    }
    func parsed(_ text: String) -> JSONValue? {
        if integer { return UInt64(text).map(JSONValue.unsigned) }
        guard let n = Double(text), n.isFinite else { return nil }
        return .number(n)
    }
    func stepped(up: Bool) -> JSONValue? {
        if let n = integerValue {
            let (next, overflow) = up ? n.addingReportingOverflow(1) : n.subtractingReportingOverflow(1)
            return overflow ? nil : .unsigned(next)
        }
        let next = value + (up ? step : -step)
        return next.isFinite ? .number(next) : nil
    }
}

struct SceneObject: Decodable, Identifiable, Sendable {
    let id: String
    let kind: String
    let index: Int
    let label: String
    let name: String
    let role: String
    let x: Double
    let z: Double
    let width: Double
    let depth: Double
    let yaw: Double
    let radius: Double
    let fields: [SceneField]
    var rectangular: Bool { kind == "wall" || kind == "tree" || role == "Door" }
    var target: JSONValue { .object(["kind": .string(kind), "index": .number(Double(index))]) }
    func value(_ field: String) -> Double { fields.first { $0.key == field }?.value ?? 0 }
}

struct SceneDocument: Decodable, Sendable {
    let document: JSONValue
    let name: String
    let objects: [SceneObject]
    let selected: String?
    let path: String?
    let revision: String?
    let arena_half: Double
    let player_radius: Double
    let projection: SceneProjection
    let lighting: [SceneField]
    let encounters: [SceneEncounter]
    let rewards: [SceneReward]
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
    mutating func fit(_ document: SceneDocument, objectIDs: Set<String>? = nil) {
        var points: [(Double, Double)] = [(0, 0)]
        for o in document.objects where objectIDs == nil || objectIDs!.contains(o.id) {
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
            if o.rectangular {
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

/// Derived relationship inspector data. Action payloads remain game-owned JSON.
struct SceneReward: Decodable, Identifiable, Sendable {
    let body: Int
    let label: String
    let action: JSONValue
    var id: String { "body:\(body)" }
}

struct SceneEncounter: Decodable, Identifiable, Sendable {
    let index: Int
    let name: String
    let trigger: Int
    let sources: [Int]
    let count: UInt32
    let within_ticks: UInt32
    let on_start: [JSONValue]
    let on_success: [JSONValue]
    let on_failure: [JSONValue]
    var id: Int { index }
    func objectIDs(rewards: [SceneReward]) -> Set<String> {
        let actions = on_start + on_success + on_failure
        return Set(["body:\(trigger)"] + sources.map { "source:\($0)" }
            + rewards.filter { actions.contains($0.action) }.map(\.id))
    }
}

/// An incomplete form is local UI state until its complete transaction is applied.
struct EncounterForm: Identifiable {
    let id = UUID()
    let index: Int?
    var name: String
    var trigger: Int?
    var sources: [Int]
    var count: String
    var ticks: String
    var onStart: [JSONValue]
    var onSuccess: [JSONValue]
    var onFailure: [JSONValue]

    init(document: SceneDocument, encounter: SceneEncounter?) {
        index = encounter?.index
        name = encounter?.name ?? "New encounter"
        trigger = encounter?.trigger ?? document.objects.first { $0.kind == "body" && $0.value("interactable") != 0 }?.index
        sources = encounter?.sources ?? []
        count = String(encounter?.count ?? 30)
        ticks = String(encounter?.within_ticks ?? 1800)
        onStart = encounter?.on_start ?? []; onSuccess = encounter?.on_success ?? []; onFailure = encounter?.on_failure ?? []
    }
    var edit: JSONValue? {
        guard let trigger, !sources.isEmpty, let count = UInt32(count), count > 0,
              let ticks = UInt32(ticks), ticks > 0 else { return nil }
        return .object(["action": .string("encounter"), "index": index.map { .number(Double($0)) } ?? .null,
            "value": .object(["name": .string(name), "trigger": .number(Double(trigger)),
                "sources": .array(sources.map { .number(Double($0)) }), "count": .unsigned(UInt64(count)),
                "within_ticks": .unsigned(UInt64(ticks)), "on_start": .array(onStart),
                "on_success": .array(onSuccess), "on_failure": .array(onFailure)])])
    }
}
