import Foundation
import Combine

struct SceneFile: Identifiable, Sendable {
    let path: String
    let label: String
    var id: String { path }
}


@MainActor
final class Model: ObservableObject {
    @Published private(set) var state: Snapshot?
    @Published private(set) var connected = false
    @Published private(set) var busy = false
    @Published private(set) var notice = "Waiting for the game…"
    @Published private(set) var failed = false
    @Published private(set) var editor: SceneDocument?
    @Published private(set) var editorNotice = "Open a scene or create one. Editing works without the game."
    @Published private(set) var editorFailed = false
    @Published private(set) var sceneFiles: [SceneFile] = []
    @Published private(set) var sceneFileError: String?
    let sceneDirectory: URL
    var canAuthor: Bool { !busy }
    @Published var editorSelection: String?
    @Published var editorEncounter: Int?
    @Published private(set) var editorPath: String?
    @Published private(set) var editorRevision: String?
    @Published private(set) var undoScenes: [SceneDocument] = []
    @Published private(set) var redoScenes: [SceneDocument] = []
    private var savedScene: JSONValue?
    var editorDirty: Bool { editor.map { $0.document != savedScene } ?? false }
    @Published var drafts: [String: String] = [:]
    var editable: Bool { connected && !busy && state?.busy == false }
    let socketPath: String
    private var inFlight = false
    private var refreshFinished: CheckedContinuation<Void, Never>?
    private let exchange: @Sendable (Request) async throws -> Reply

    private let author: @Sendable (EditorRequest) async throws -> AuthoringReply
    init(path: String, sceneDirectory: URL? = nil, exchange: (@Sendable (Request) async throws -> Reply)? = nil,
         author: (@Sendable (EditorRequest) async throws -> AuthoringReply)? = nil) {
        socketPath = path
        let connection = Connection(path: path)
        self.exchange = exchange ?? { try await connection.exchange($0) }
        let authoring = Authoring()
        self.author = author ?? { try await authoring.exchange($0) }
        let args = CommandLine.arguments
        let path = args.firstIndex(of: "--scenes").flatMap { args.indices.contains($0 + 1) ? args[$0 + 1] : nil }
        self.sceneDirectory = sceneDirectory ?? URL(fileURLWithPath: path ?? "scenes", relativeTo: URL(fileURLWithPath: FileManager.default.currentDirectoryPath)).standardizedFileURL
        refreshSceneFiles()
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
        return await sendAuthoring(EditorRequest(action: "edit", document: editor.document, edit: edit))
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
        if action == "new" || action == "open" { editorEncounter = document.encounters.first?.index }
        if case .object(let edit) = request.edit {
            if edit["action"] == .string("delete_encounter") { editorEncounter = nil }
        }
        if action == "edit" {
            // Deletion shifts array indices: a surviving ID can now name a
            // different object. Clearing selection also discards its inspector drafts.
            if case .object(let edit) = request.edit, edit["action"] == .string("delete") {
                editorSelection = nil
            } else {
                editorSelection = document.selected ?? editorSelection.flatMap { id in document.objects.contains { $0.id == id } ? id : nil }
            }
        } else if action != "save" { editorSelection = document.selected ?? document.objects.first?.id }
        if action == "new" || action == "open" || action == "save" {
            editorPath = document.path; editorRevision = document.revision
            savedScene = action == "new" ? nil : document.document
        }
    }

    func undoScene() {
        guard !busy, let previous = undoScenes.popLast() else { return }
        if let editor { redoScenes.append(editor) }
        editor = previous; editorSelection = previous.selected; editorEncounter = nil
    }

    func redoScene() {
        guard !busy, let next = redoScenes.popLast() else { return }
        if let editor { undoScenes.append(editor) }
        editor = next; editorSelection = next.selected; editorEncounter = nil
    }

    func saveScene(path: String, play: Bool) async {
        guard let editor, !play || editable else { return }
        let saved = await sendAuthoring(EditorRequest(action: "save", document: editor.document,
            path: path, revision: path == editorPath ? editorRevision : nil))
        guard saved else { return }
        refreshSceneFiles()
        if play {
            if await send(Request(op: .startScene, path: path)) {
                editorNotice = "Scene saved and started."
            } else {
                editorFailed = true; editorNotice = "Scene saved, but play failed: \(notice)"
            }
        }
    }

    @discardableResult
    func sendAuthoring(_ request: EditorRequest) async -> Bool {
        guard canAuthor else { return false }
        busy = true
        defer { busy = false }
        do {
            let reply = try await author(request)
            guard reply.ok, let document = reply.editor else {
                throw ConnectionError(message: reply.error ?? "Authoring returned no document.")
            }
            acceptEditor(document, request: request)
            editorFailed = false
            editorNotice = request.action == "save" ? "Scene saved." : "Draft ready. Save & play to try it."
            return true
        } catch {
            editorFailed = true; editorNotice = error.localizedDescription
            return false
        }
    }

    func refreshSceneFiles() {
        do {
            sceneFiles = try FileManager.default.contentsOfDirectory(at: sceneDirectory,
                includingPropertiesForKeys: [.isRegularFileKey]).filter {
                    $0.pathExtension == "ron" && (try? $0.resourceValues(forKeys: [.isRegularFileKey]).isRegularFile) == true
                }.sorted { $0.lastPathComponent < $1.lastPathComponent }.map { SceneFile(path: $0.path, label: $0.lastPathComponent) }
            sceneFileError = nil
        } catch { sceneFiles = []; sceneFileError = error.localizedDescription }
    }
}
