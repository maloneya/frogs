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
                                tolerance: 8 / viewport.scale(width: geometry.size.width, height: geometry.size.height))
                            selection = dragObject?.id
                        }
                    }
                    dragEnd = value.location
                    if tool == "pan", let startViewport {
                        let scale = startViewport.scale(width: geometry.size.width, height: geometry.size.height)
                        viewport.centreX = startViewport.centreX - value.translation.width / scale
                        viewport.centreZ = startViewport.centreZ + value.translation.height / scale
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
                    case "body", "source", "grid": edit(.object(["action": .string("add"), "kind": .string(tool), "pos": .point(x, z)]))
                    default: break
                    }
                })
                .accessibilityLabel("Scene map. Select objects and edit precise coordinates in the inspector. X increases right; Z increases up.")
        }.frame(minHeight: 300)
    }
    private func point(_ point: CGPoint, _ size: CGSize, snap: Double) -> (Double, Double) {
        viewport.world(x: point.x, y: point.y, width: size.width, height: size.height, snap: snap)
    }
    private func moved(_ object: SceneObject, size: CGSize, translation: CGSize) -> (Double, Double) {
        let scale = viewport.scale(width: size.width, height: size.height)
        let x = object.x + translation.width / scale, z = object.z - translation.height / scale
        return viewport.snapped(x: x, z: z, spacing: snap)
    }
    private func draw(_ context: GraphicsContext, _ size: CGSize) {
        let scale = viewport.scale(width: size.width, height: size.height)
        func screen(_ x: Double, _ z: Double) -> CGPoint {
            let (a, b) = viewport.screen(x: x, z: z, width: size.width, height: size.height)
            return CGPoint(x: a, y: b)
        }
        func circle(_ x: Double, _ z: Double, _ radius: Double) -> Path {
            let p = screen(x, z), r = max(3, radius * scale)
            return Path(ellipseIn: CGRect(x: p.x - r, y: p.y - r, width: r * 2, height: r * 2))
        }
        let grid = scale > 24 ? 1.0 : scale > 6 ? 5.0 : 10.0
        let (left, top) = point(.zero, size, snap: 0)
        let (right, bottom) = point(CGPoint(x: size.width, y: size.height), size, snap: 0)
        var lines = Path()
        for x in stride(from: (left / grid).rounded(.down) * grid, through: right, by: grid) {
            lines.move(to: screen(x, top)); lines.addLine(to: screen(x, bottom))
        }
        for z in stride(from: (bottom / grid).rounded(.down) * grid, through: top, by: grid) {
            lines.move(to: screen(left, z)); lines.addLine(to: screen(right, z))
        }
        context.stroke(lines, with: .color(.secondary.opacity(0.18)), lineWidth: 0.5)
        let arena = document.arena_half, a = screen(-arena, arena), b = screen(arena, -arena)
        context.stroke(Path(CGRect(x: a.x, y: a.y, width: b.x-a.x, height: b.y-a.y)), with: .color(.secondary), lineWidth: 2)
        var axes = Path(); axes.move(to: screen(left, 0)); axes.addLine(to: screen(right, 0)); axes.move(to: screen(0, top)); axes.addLine(to: screen(0, bottom))
        context.stroke(axes, with: .color(.secondary.opacity(0.45)), lineWidth: 1)
        for marker in document.trees + document.torches {
            context.fill(circle(marker.x, marker.z, 0.4), with: .color(.secondary.opacity(0.3)))
            if scale > 8 { context.draw(Text(marker.label).font(.caption2).foregroundStyle(.secondary), at: screen(marker.x, marker.z - 1)) }
        }
        var formationMarkersRemaining = 2000
        for object in document.objects {
            var x = object.x, z = object.z
            if editable, object.id == dragObject?.id, let start = dragStart, let end = dragEnd {
                (x, z) = moved(object, size: size, translation: CGSize(width: end.x-start.x, height: end.y-start.y))
            }
            let selected = object.id == selection
            let colour: Color = selected ? .accentColor : object.kind == "source" ? .orange : .primary
            var path = Path()
            switch object.kind {
            case "wall":
                for (i, corner) in [(-1.0,-1.0),(1.0,-1.0),(1.0,1.0),(-1.0,1.0)].enumerated() {
                    let a = corner.0 * object.width / 2, b = corner.1 * object.depth / 2
                    let p = screen(x + a*cos(object.yaw) + b*sin(object.yaw), z - a*sin(object.yaw) + b*cos(object.yaw))
                    if i == 0 { path.move(to: p) } else { path.addLine(to: p) }
                }
                path.closeSubpath(); context.fill(path, with: .color(colour.opacity(0.25)))
            case "grid":
                let columns = max(1, Int(object.value("columns"))), rows = max(1, Int(object.value("rows"))), spacing = object.value("spacing")
                // Bound total preview work, even with many large formations.
                if columns * rows <= formationMarkersRemaining {
                    formationMarkersRemaining -= columns * rows
                    for row in 0..<rows { for column in 0..<columns {
                        context.fill(circle(x + Double(column)*spacing, z + Double(row)*spacing, object.radius), with: .color(colour.opacity(0.5)))
                    } }
                }
                let a=screen(x-object.radius,z+object.depth+object.radius), b=screen(x+object.width+object.radius,z-object.radius)
                path=Path(CGRect(x:a.x,y:a.y,width:b.x-a.x,height:b.y-a.y))
            default:
                path = circle(x,z,object.radius)
                if object.kind == "source" {
                    context.fill(circle(x,z,0.3), with: .color(colour))
                } else { context.fill(path, with: .color(colour.opacity(0.35))) }
            }
            context.stroke(path, with: .color(colour), style: StrokeStyle(lineWidth: selected ? 2 : 1, dash: object.kind == "source" ? [4,3] : []))
            if selected || (object.kind != "body" && scale > 6) {
                context.draw(Text(object.label).font(.caption).foregroundStyle(colour), at: screen(x,z - max(object.radius,object.depth/2) - 1))
            }
        }
        context.fill(circle(0,0,document.player_radius), with: .color(.green))
        context.draw(Text("P").font(.caption.bold()).foregroundStyle(.green), at: screen(0,-1))
        if tool == "wall", let start = dragStart, let end = dragEnd {
            let (x,z)=point(start,size,snap:snap), (a,b)=point(end,size,snap:snap)
            let p=screen(min(x,a),max(z,b)), q=screen(max(x,a),min(z,b))
            context.stroke(Path(CGRect(x:p.x,y:p.y,width:max(1,q.x-p.x),height:max(1,q.y-p.y))), with:.color(.accentColor),lineWidth:2)
        }
        context.draw(Text("Grid \(grid.formatted()) m").font(.caption).foregroundStyle(.secondary), at:CGPoint(x:12,y:12),anchor:.topLeading)
    }
}
