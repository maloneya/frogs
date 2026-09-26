import Foundation
import Combine

@MainActor
final class Model: ObservableObject {
    @Published private(set) var state: Snapshot?
    @Published private(set) var connected = false
    @Published private(set) var busy = false
    @Published private(set) var notice = "Waiting for the game…"
    @Published private(set) var failed = false
    @Published private(set) var editor: SceneDocument?
    @Published var editorSelection: String?
    @Published private(set) var editorPath: String?
    @Published private(set) var editorRevision: String?
    @Published private(set) var undoScenes: [SceneDocument] = []
    @Published private(set) var redoScenes: [SceneDocument] = []
    private var savedScene: JSONValue?
    var editorDirty: Bool { editor.map { $0.scene != savedScene } ?? false }
    @Published var drafts: [String: String] = [:]
    var editable: Bool { connected && !busy && state?.busy == false }
    let socketPath: String
    private var inFlight = false
    private var refreshFinished: CheckedContinuation<Void, Never>?
    private let exchange: @Sendable (Request) async throws -> Reply

    init(path: String, exchange: (@Sendable (Request) async throws -> Reply)? = nil) {
        socketPath = path
        let connection = Connection(path: path)
        self.exchange = exchange ?? { try await connection.exchange($0) }
    }

    func poll() async {
        while !Task.isCancelled {
            if !inFlight { await send(Request(op: .state)) }
            try? await Task.sleep(for: .milliseconds(500))
        }
    }

    func submit(_ field: Attribute) async {
        guard let draft = drafts[field.key] else { return }
        guard let value = Double(draft.trimmingCharacters(in: .whitespaces)), value.isFinite else {
            notice = "Enter a finite number for \(field.label)."
            failed = true
            return
        }
        await set(field, value: value)
    }

    func set(_ field: Attribute, value: Double) async {
        let submittedDraft = drafts[field.key]
        if await send(Request(op: .set, key: field.key, value: value)), drafts[field.key] == submittedDraft {
            drafts.removeValue(forKey: field.key)
        }
    }

    func action(_ op: Operation) async {
        if await send(Request(op: op)), op == .revert || op == .defaults {
            drafts.removeAll()
        }
    }

    @discardableResult
    func send(_ request: Request) async -> Bool {
        let editing = request.op != .state
        guard !busy, !editing || connected else { return false }
        // Reserve the sole edit slot before awaiting an in-flight refresh. This
        // disables controls immediately and prevents reordering queued clicks.
        if editing { busy = true }
        defer { if editing { busy = false } }
        var command = request
        if editing { command.session = state?.session }
        if inFlight {
            guard editing else { return false }
            await withCheckedContinuation { refreshFinished = $0 }
        }
        guard !editing || connected else { return false }
        inFlight = true
        defer {
            inFlight = false
            refreshFinished?.resume()
            refreshFinished = nil
        }
        do {
            let reply = try await exchange(command)
            guard let snapshot = reply.state else {
                throw ConnectionError(message: reply.error ?? "Game returned no state")
            }
            let reconnecting = !connected || state?.session != snapshot.session
            if reconnecting { drafts.removeAll() }
            state = snapshot
            connected = true
            if reply.ok, let document = reply.editor, let request = request.request {
                acceptEditor(document, request: request)
            }
            // Polls must not erase a local validation error while its draft remains.
            if request.op != .state || reconnecting || drafts.isEmpty {
                notice = reply.error ?? snapshot.message
                failed = !reply.ok || snapshot.error
            }
            return reply.ok
        } catch {
            connected = false
            drafts.removeAll()
            notice = "Disconnected. Reconnecting… \(error.localizedDescription)"
            failed = true
            return false
        }
    }
}


extension Model {
    @discardableResult
    func editScene(_ edit: JSONValue) async -> Bool {
        guard let editor else { return false }
        return await send(Request(op: .sceneEditor, request: EditorRequest(action: "edit", scene: editor.scene, edit: edit)))
    }

    func acceptEditor(_ document: SceneDocument, request: EditorRequest) {
        let action = request.action
        if action == "edit" {
            if let editor { undoScenes.append(editor); if undoScenes.count > 32 { undoScenes.removeFirst() } }
            redoScenes.removeAll()
        } else if action == "new" || action == "open" {
            undoScenes.removeAll(); redoScenes.removeAll()
        }
        editor = document
        if action == "edit" {
            // Deletion shifts array indices: a surviving ID can now name a
            // different object. Clearing selection also discards its inspector drafts.
            if case .object(let edit) = request.edit, edit["action"] == .string("delete") {
                editorSelection = nil
            } else {
                editorSelection = document.selected ?? editorSelection.flatMap { id in document.objects.contains { $0.id == id } ? id : nil }
            }
        } else if action != "save" { editorSelection = document.selected ?? document.objects.first?.id }
        if action != "edit" {
            editorPath = document.path; editorRevision = document.revision
            savedScene = action == "new" ? nil : document.scene
        }
    }

    func undoScene() {
        guard !busy, let previous = undoScenes.popLast() else { return }
        if let editor { redoScenes.append(editor) }
        editor = previous; editorSelection = previous.selected
    }

    func redoScene() {
        guard !busy, let next = redoScenes.popLast() else { return }
        if let editor { undoScenes.append(editor) }
        editor = next; editorSelection = next.selected
    }

    func saveScene(path: String, play: Bool) async {
        guard let editor else { return }
        await send(Request(op: .sceneEditor, request: EditorRequest(action: "save", scene: editor.scene,
            path: path, revision: path == editorPath ? editorRevision : nil, play: play)))
    }
}
