import AppKit
import SwiftUI

struct ScenePointerDrag {
    let start: CGPoint
    let location: CGPoint
    let rightButton: Bool
    let began: Bool
    var translation: CGSize { CGSize(width: location.x - start.x, height: location.y - start.y) }
}

// SwiftUI's DragGesture only handles the primary mouse button. Keep both
// buttons on the same responder path, and leave text editing to its responder.
struct SceneCanvasInput: NSViewRepresentable {
    let changed: (ScenePointerDrag) -> Void
    let ended: (ScenePointerDrag) -> Void
    let deleteSelection: () -> Void

    func makeNSView(context: Context) -> ScenePointerView { ScenePointerView() }
    func updateNSView(_ view: ScenePointerView, context: Context) {
        view.changed = changed
        view.ended = ended
        view.deleteSelection = deleteSelection
    }
}

final class ScenePointerView: NSView {
    var changed: (ScenePointerDrag) -> Void = { _ in }
    var ended: (ScenePointerDrag) -> Void = { _ in }
    var deleteSelection: () -> Void = {}
    private var start: CGPoint?
    private var button: Int?
    override var isFlipped: Bool { true }
    override var acceptsFirstResponder: Bool { true }

    override func mouseDown(with event: NSEvent) { begin(event) }
    override func rightMouseDown(with event: NSEvent) { begin(event) }
    override func mouseDragged(with event: NSEvent) { update(event, ending: false) }
    override func rightMouseDragged(with event: NSEvent) { update(event, ending: false) }
    override func mouseUp(with event: NSEvent) { update(event, ending: true) }
    override func rightMouseUp(with event: NSEvent) { update(event, ending: true) }

    private func begin(_ event: NSEvent) {
        guard button == nil else { return }
        window?.makeFirstResponder(self)
        let point = convert(event.locationInWindow, from: nil)
        start = point
        button = event.buttonNumber
        changed(ScenePointerDrag(start: point, location: point, rightButton: button == 1, began: true))
    }
    private func update(_ event: NSEvent, ending: Bool) {
        guard let start, button == event.buttonNumber else { return }
        let drag = ScenePointerDrag(start: start, location: convert(event.locationInWindow, from: nil),
                                    rightButton: button == 1, began: false)
        if ending {
            self.start = nil
            button = nil
            ended(drag)
        } else { changed(drag) }
    }
    override func keyDown(with event: NSEvent) {
        let modifiers = event.modifierFlags.intersection([.command, .control, .option, .shift])
        if (event.keyCode == 51 || event.keyCode == 117), modifiers.isEmpty {
            // Ignore repeat and active drags so one press cannot delete a newly
            // reindexed object or leave a move targeting a deleted placement.
            if !event.isARepeat, button == nil { deleteSelection() }
        } else { super.keyDown(with: event) }
    }
}
