import SwiftUI

struct SceneCanvas: View {
    let document: SceneDocument
    @Binding var selection: String?
    @Binding var viewport: SceneViewport
    let tool: String
    let snap: Double
    let editable: Bool
    let edit: (JSONValue) -> Void
    @State private var dragObject: SceneObject?
    @State private var dragStart: CGPoint?
    @State private var dragEnd: CGPoint?
    @State private var startViewport: SceneViewport?

    var body: some View {
        GeometryReader { geometry in
            Canvas { context, size in draw(context, size) }
                .background(Color(nsColor: .textBackgroundColor))
                .clipped().contentShape(Rectangle())
                .gesture(DragGesture(minimumDistance: 0).onChanged { value in
                    if dragStart == nil {
                        dragStart = value.startLocation; startViewport = viewport
                        if tool == "select" {
                            let (x, z) = point(value.startLocation, geometry.size, snap: 0)
                            dragObject = viewport.hit(document.objects, x: x, z: z,
                                tolerance: document.projection.worldTolerance(8 / viewport.scale(width: geometry.size.width, height: geometry.size.height)))
                            selection = dragObject?.id
                        }
                    }
                    dragEnd = value.location
                    if tool == "pan", let startViewport {
                        let (dx, dz) = startViewport.delta(x: value.translation.width, y: value.translation.height,
                            width: geometry.size.width, height: geometry.size.height, projection: document.projection)
                        viewport.centreX = startViewport.centreX - dx
                        viewport.centreZ = startViewport.centreZ - dz
                    }
                }.onEnded { value in
                    defer { dragStart = nil; dragEnd = nil; dragObject = nil; startViewport = nil }
                    guard editable else { return }
                    let (x, z) = point(value.location, geometry.size, snap: snap)
                    switch tool {
                    case "select":
                        if let object = dragObject, hypot(value.translation.width, value.translation.height) > 2 {
                            let (a, b) = moved(object, size: geometry.size, translation: value.translation)
                            edit(.object(["action": .string("move"), "target": object.target, "pos": .point(a, b)]))
                        }
                    case "wall":
                        if hypot(value.translation.width, value.translation.height) > 3 {
                            let (a, b) = point(value.startLocation, geometry.size, snap: snap)
                            edit(.object(["action": .string("draw_wall"), "start": .point(a, b), "end": .point(x, z)]))
                        } else { edit(.object(["action": .string("add"), "kind": .string(tool), "pos": .point(x, z)])) }
                    case "body", "source", "grid", "tree", "campfire": edit(.object(["action": .string("add"), "kind": .string(tool), "pos": .point(x, z)]))
                    default: break
                    }
                })
                .accessibilityLabel("Scene map. Select objects and edit precise coordinates in the inspector. The map follows the game camera; the grid shows world X and Z.")
        }.frame(minHeight: 300)
    }
    private func point(_ point: CGPoint, _ size: CGSize, snap: Double) -> (Double, Double) {
        viewport.world(x: point.x, y: point.y, width: size.width, height: size.height, snap: snap, projection: document.projection)
    }
    private func moved(_ object: SceneObject, size: CGSize, translation: CGSize) -> (Double, Double) {
        let (dx, dz) = viewport.delta(x: translation.width, y: translation.height,
            width: size.width, height: size.height, projection: document.projection)
        return viewport.snapped(x: object.x + dx, z: object.z + dz, spacing: snap)
    }
    private func draw(_ context: GraphicsContext, _ size: CGSize) {
        let scale = viewport.scale(width: size.width, height: size.height)
        func screen(_ x: Double, _ z: Double) -> CGPoint {
            let (a, b) = viewport.screen(x: x, z: z, width: size.width, height: size.height, projection: document.projection)
            return CGPoint(x: a, y: b)
        }
        func polygon(_ points: [(Double, Double)]) -> Path {
            var path = Path()
            for (index, point) in points.enumerated() {
                let p = screen(point.0, point.1)
                if index == 0 { path.move(to: p) } else { path.addLine(to: p) }
            }
            path.closeSubpath()
            return path
        }
        func rectangle(_ x0: Double, _ z0: Double, _ x1: Double, _ z1: Double) -> Path {
            polygon([(x0,z0),(x1,z0),(x1,z1),(x0,z1)])
        }
        func circle(_ x: Double, _ z: Double, _ radius: Double) -> Path {
            // Project the footprint itself: world circles become ground ellipses.
            let p = screen(x, z), projection = document.projection
            let transform = CGAffineTransform(a: projection.right[0] * scale, b: -projection.up[0] * scale,
                c: projection.right[1] * scale, d: -projection.up[1] * scale, tx: p.x, ty: p.y)
            return Path(ellipseIn: CGRect(x: -radius, y: -radius, width: radius * 2, height: radius * 2)).applying(transform)
        }
        func marker(_ x: Double, _ z: Double) -> Path {
            let p = screen(x, z)
            return Path(ellipseIn: CGRect(x: p.x - 3, y: p.y - 3, width: 6, height: 6))
        }
        // Inverse-project every screen corner; two diagonal corners are no
        // longer enough to bound the rotated visible ground rectangle.
        let corners = [CGPoint.zero, CGPoint(x: size.width,y: 0), CGPoint(x: size.width,y: size.height), CGPoint(x: 0,y: size.height)]
            .map { point($0, size, snap: 0) }
        let left = corners.map { $0.0 }.min()!, right = corners.map { $0.0 }.max()!
        let bottom = corners.map { $0.1 }.min()!, top = corners.map { $0.1 }.max()!
        let grid = SceneViewport.gridSpacing(extent: max(right-left, top-bottom))
        var lines = Path()
        for x in stride(from: (left / grid).rounded(.down) * grid, through: right, by: grid) {
            lines.move(to: screen(x, top)); lines.addLine(to: screen(x, bottom))
        }
        for z in stride(from: (bottom / grid).rounded(.down) * grid, through: top, by: grid) {
            lines.move(to: screen(left, z)); lines.addLine(to: screen(right, z))
        }
        context.stroke(lines, with: .color(.secondary.opacity(0.18)), lineWidth: 0.5)
        let arena = document.arena_half
        context.stroke(rectangle(-arena,-arena,arena,arena), with: .color(.secondary), lineWidth: 2)
        var axes = Path(); axes.move(to: screen(left, 0)); axes.addLine(to: screen(right, 0)); axes.move(to: screen(0, top)); axes.addLine(to: screen(0, bottom))
        context.stroke(axes, with: .color(.secondary.opacity(0.45)), lineWidth: 1)
        var formationMarkersRemaining = 2000
        for object in document.objects {
            var x = object.x, z = object.z
            if editable, object.id == dragObject?.id, let start = dragStart, let end = dragEnd {
                (x, z) = moved(object, size: size, translation: CGSize(width: end.x-start.x, height: end.y-start.y))
            }
            let selected = object.id == selection
            let colour: Color = selected ? .accentColor : object.kind == "tree" ? .green : ["source", "campfire"].contains(object.kind) ? .orange : .primary
            var path = Path()
            switch object.kind {
            case "wall", "tree":
                for (i, corner) in [(-1.0,-1.0),(1.0,-1.0),(1.0,1.0),(-1.0,1.0)].enumerated() {
                    let a = corner.0 * object.width / 2, b = corner.1 * object.depth / 2
                    let p = screen(x + a*cos(object.yaw) + b*sin(object.yaw), z - a*sin(object.yaw) + b*cos(object.yaw))
                    if i == 0 { path.move(to: p) } else { path.addLine(to: p) }
                }
                path.closeSubpath(); context.fill(path, with: .color(colour.opacity(0.25)))
                if object.kind == "tree" {
                    context.draw(Text(Image(systemName: "tree.fill")).font(.caption).foregroundStyle(colour), at: screen(x,z))
                }
            case "grid":
                let columns = max(1, Int(object.value("columns"))), rows = max(1, Int(object.value("rows"))), spacing = object.value("spacing")
                // Bound total preview work, even with many large formations.
                if columns * rows <= formationMarkersRemaining {
                    formationMarkersRemaining -= columns * rows
                    for row in 0..<rows { for column in 0..<columns {
                        context.fill(circle(x + Double(column)*spacing, z + Double(row)*spacing, object.radius), with: .color(colour.opacity(0.5)))
                    } }
                }
                path = rectangle(x-object.radius,z-object.radius,x+object.width+object.radius,z+object.depth+object.radius)
            default:
                path = circle(x,z,object.radius)
                if object.kind == "campfire" {
                    context.draw(Text(Image(systemName: "flame.fill")).font(.caption).foregroundStyle(colour), at: screen(x,z))
                } else if object.kind == "source" {
                    context.fill(marker(x,z), with: .color(colour))
                } else {
                    context.fill(path, with: .color(colour.opacity(0.35)))
                    context.fill(marker(x,z), with: .color(colour.opacity(0.5)))
                }
            }
            context.stroke(path, with: .color(colour), style: StrokeStyle(lineWidth: selected ? 2 : 1, dash: ["source", "campfire"].contains(object.kind) ? [4,3] : []))
            if selected || (object.kind != "body" && scale > 6) {
                let anchor = screen(x, z)
                context.draw(Text(object.label).font(.caption).foregroundStyle(colour), at: CGPoint(x: anchor.x, y: anchor.y + 15))
            }
        }
        context.fill(circle(0,0,document.player_radius), with: .color(.green))
        context.draw(Text("P").font(.caption.bold()).foregroundStyle(.green), at: screen(0,-1))
        if tool == "wall", let start = dragStart, let end = dragEnd {
            let (x,z)=point(start,size,snap:snap), (a,b)=point(end,size,snap:snap)
            context.stroke(rectangle(min(x,a),min(z,b),max(x,a),max(z,b)), with:.color(.accentColor),lineWidth:2)
        }
        context.draw(Text("Game camera · Grid \(grid.formatted()) m").font(.caption).foregroundStyle(.secondary), at:CGPoint(x:12,y:12),anchor:.topLeading)
        let compass = CGPoint(x: size.width - 48, y: 32)
        for (label, x, z) in [("+X", 1.0, 0.0), ("+Z", 0.0, 1.0)] {
            let (a,b) = document.projection.project(x: x, z: z)
            let end = CGPoint(x: compass.x + a * 32, y: compass.y - b * 32)
            var axis = Path(); axis.move(to: compass); axis.addLine(to: end)
            context.stroke(axis, with: .color(.secondary), lineWidth: 1)
            context.draw(Text(label).font(.caption2).foregroundStyle(.secondary), at: CGPoint(x:end.x,y:end.y+9))
        }
    }
}
