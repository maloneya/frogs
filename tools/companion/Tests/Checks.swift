import Foundation

// A synthetic catalog intentionally uses an unknown field: UI must not depend on
// the current game's seven keys, groups, ranges or defaults.
private func reply(value: Double = 9, session: UInt32 = 1, ok: Bool = true) throws -> Reply {
    let data = """
    {"ok":\(ok),"error":\(ok ? "null" : "\"Out of range\""),"state":{
    "session":\(session),"fields":[{"key":"future_field","group":"Future type","label":"Speed","unit":"m/s","default":9,"min":0,"max":60,"step":0.1,"value":\(value),"saved":9}],
    "unsaved":\(value != 9),"path":"tuning.ron","message":"Ready","error":false,"busy":false,
    "scenes":{"files":[{"path":"scenes/a.ron","label":"a.ron"}],"error":null,"current":"Test","run":1,"tick":42},
    "debug":{"collision":false,"vsync":true,"view_height":30,"fps":60,"frame_ms":16.67,"enemies":3,"instances":4,
    "capture":{"pending":false,"path":null,"error":null}}}}
    """.data(using: .utf8)!
    return try JSONDecoder().decode(Reply.self, from: data)
}

private actor Peer {
    var value: Double = 9
    var session: UInt32 = 1
    var offline = false
    var edits = 0
    private var pauseRead = false
    private var readGate: CheckedContinuation<Void, Never>?
    func pauseNextRead() { pauseRead = true }
    func readIsPaused() -> Bool { readGate != nil }
    func resumeRead() { readGate?.resume(); readGate = nil }
    func restart() { session += 1; value = 9; offline = false }
    func disconnect() { offline = true }
    func exchange(_ request: Request) async throws -> Reply {
        if request.op == .state, pauseRead {
            pauseRead = false
            await withCheckedContinuation { readGate = $0 }
        }
        if offline { throw ConnectionError(message: "Stopped") }
        if request.op != .state, request.session != session {
            return try reply(value: value, session: session, ok: false)
        }
        if request.op == .set {
            edits += 1
            if let next = request.value, next >= 0 { value = next }
            else { return try reply(value: value, session: session, ok: false) }
        }
        return try reply(value: value, session: session)
    }
}

@MainActor
func draftsSurviveRefreshButNeverReconnect() async throws {
    let peer = Peer()
    let model = Model(path: "unused", exchange: { try await peer.exchange($0) })
    await model.send(Request(op: .state))
    precondition(model.state?.groups == ["Future type"])
    let field = model.state!.fields.first!
    model.drafts[field.key] = "12.5"
    await model.send(Request(op: .state))
    precondition(model.drafts[field.key] == "12.5")
    await model.submit(field)
    precondition(model.state?.fields.first?.value == 12.5)
    precondition(model.drafts.isEmpty)
    model.drafts[field.key] = "-1"
    await model.submit(field)
    precondition(model.failed)
    precondition(model.state?.fields.first?.value == 12.5)
    precondition(model.drafts[field.key] == "-1")
    await peer.disconnect()
    await model.send(Request(op: .state))
    precondition(!model.connected)
    precondition(model.drafts.isEmpty)
    await model.set(field, value: 30)
    let editCount = await peer.edits
    precondition(editCount == 2) // no offline replay
    await peer.restart()
    await model.send(Request(op: .state))
    precondition(model.connected)
    precondition(model.state?.fields.first?.value == 9)
    precondition(model.state?.session == 2)
    model.drafts[field.key] = "40"
    await peer.restart() // restart occurred between refresh and submit
    await model.submit(field)
    precondition(model.state?.fields.first?.value == 9)
    precondition(model.drafts.isEmpty)
    precondition(model.failed)
}

@MainActor
func invalidTextNeverLeavesTheCompanion() async throws {
    let peer = Peer()
    let model = Model(path: "unused", exchange: { try await peer.exchange($0) })
    await model.send(Request(op: .state))
    let field = model.state!.fields.first!
    for invalid in ["-", "NaN", "inf", "hello"] {
        model.drafts[field.key] = invalid
        await model.submit(field)
        precondition(model.failed)
        await model.send(Request(op: .state))
        precondition(model.failed)
        precondition(model.drafts[field.key] == invalid)
    }
    let editCount = await peer.edits
    precondition(editCount == 0)
}

@MainActor
func editAdmissionWaitsForRefreshWithoutQueuingMoreEdits() async {
    let peer = Peer()
    let model = Model(path: "unused", exchange: { try await peer.exchange($0) })
    await model.send(Request(op: .state))
    let field = model.state!.fields.first!
    await peer.pauseNextRead()
    let refresh = Task { await model.send(Request(op: .state)) }
    for _ in 0..<1000 {
        if await peer.readIsPaused() { break }
        await Task.yield()
    }
    let paused = await peer.readIsPaused()
    precondition(paused)
    model.drafts[field.key] = "12"
    let edit = Task { await model.submit(field) }
    for _ in 0..<1000 {
        if model.busy { break }
        await Task.yield()
    }
    precondition(model.busy) // controls disable before the waiting edit reaches the wire
    let admitted = await model.send(Request(op: .set, key: field.key, value: 20))
    precondition(!admitted)
    model.acceptEditor(try! sceneDocument("Busy draft"), request: EditorRequest(action: "new"))
    let sceneAdmitted = await model.editScene(.object(["action": .string("rename"), "name": .string("Lost edit")]))
    precondition(!sceneAdmitted && model.editor?.name == "Busy draft")
    model.drafts[field.key] = "newer text"
    await peer.resumeRead()
    _ = await refresh.value
    await edit.value
    precondition(model.state?.fields.first?.value == 12)
    precondition(model.drafts[field.key] == "newer text")
    let count = await peer.edits
    precondition(count == 1)
}

func localTransportFailsClearlyWithoutAGame() async {
    let client = Connection(path: "/tmp/no-arpg-\(UUID().uuidString).sock")
    do {
        _ = try await client.exchange(Request(op: .state))
        fatalError("Missing game must report a connection failure")
    } catch is ConnectionError {} catch { fatalError("Unexpected error: \(error)") }
}


@MainActor
func liveToolingChecks(_ model: Model) async throws {
    let original = model.state!
    guard let file = original.scenes.files.first(where: { $0.label == "single_enemy.ron" }) else {
        fatalError("Live check requires the repository scene catalog")
    }
    await model.send(Request(op: .startScene, path: file.path))
    precondition(!model.failed && model.state!.scenes.run == original.scenes.run + 1)
    let started = model.state!.scenes
    await model.send(Request(op: .startScene, path: "/no-such-arpg-scene.ron"))
    precondition(model.failed && model.state!.scenes.run == started.run)
    precondition(model.state!.scenes.current == started.current)
    await model.action(.restart)
    precondition(!model.failed && model.state!.scenes.run == started.run + 1)
    await model.action(.refreshScenes)
    precondition(!model.failed && model.state!.scenes.files.contains { $0.path == file.path })

    let debug = model.state!.debug
    await model.send(Request(op: .collision, enabled: !debug.collision))
    precondition(!model.failed && model.state!.debug.collision != debug.collision)
    await model.send(Request(op: .collision, enabled: debug.collision))
    await model.send(Request(op: .vsync, enabled: !debug.vsync!))
    precondition(!model.failed && model.state!.debug.vsync != debug.vsync)
    await model.send(Request(op: .vsync, enabled: debug.vsync))
    await model.send(Request(op: .zoom, direction: .in))
    precondition(!model.failed && model.state!.debug.view_height! < debug.view_height!)
    await model.send(Request(op: .zoom, direction: .out))
    precondition(abs(model.state!.debug.view_height! - debug.view_height!) < 0.0001)

    let directory = FileManager.default.temporaryDirectory.appendingPathComponent("arpg-capture-\(UUID().uuidString)")
    try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
    defer { try? FileManager.default.removeItem(at: directory) }
    try Data([0]).write(to: directory.appendingPathComponent("blocked"))
    for valid in [false, true] {
        let path = directory.appendingPathComponent(valid ? "frame.png" : "blocked/frame.png").path
        await model.send(Request(op: .capture, path: path))
        precondition(!model.failed && model.state!.debug.capture.pending)
        for _ in 0..<60 {
            try await Task.sleep(for: .milliseconds(100))
            await model.send(Request(op: .state))
            if model.state?.debug.capture.pending == false { break }
        }
        precondition(model.connected && !model.state!.debug.capture.pending)
        if valid {
            precondition(model.state!.debug.capture.error == nil)
            precondition(model.state!.debug.capture.path == path)
            let data = try Data(contentsOf: URL(fileURLWithPath: path))
            precondition(data.starts(with: [137, 80, 78, 71, 13, 10, 26, 10]))
        } else {
            precondition(model.state!.debug.capture.error != nil)
            precondition(!FileManager.default.fileExists(atPath: path))
        }
    }
    print("Native companion: live scene start/refusal/restart, catalog, collision, vsync, zoom and screenshot success/failure passed")
}


@main
struct Checks {
    @MainActor static func main() async throws {
        try await draftsSurviveRefreshButNeverReconnect()
        try await invalidTextNeverLeavesTheCompanion()
        try await sceneDraftsSurviveDisconnectAndUndoAcrossSave()
        try sceneWideEditsPreserveSelection()
        try sceneJSONPreservesExactIntegers()
        try encounterFormsPreserveCompleteRelationships()
        try integerInspectorPreservesPrecision()
        sceneCoordinatesRoundTrip()
        try await liveSceneEditorChecks(Model(path: "/tmp/no-authoring-game-\(UUID().uuidString).sock"))
        await localTransportFailsClearlyWithoutAGame()
        await editAdmissionWaitsForRefreshWithoutQueuingMoreEdits()
        print("Native companion: draft, validation, reconnect and transport checks passed")
        if let path = ProcessInfo.processInfo.environment["ARPG_COMPANION_TEST_SOCKET"] {
            let model = Model(path: path)
            await model.send(Request(op: .state))
            precondition(model.connected)
            // Startup can intentionally have no scene. Restart must exercise
            // an admitted scene rather than accidentally passing a refusal.
            guard let scene = model.state!.scenes.files.first(where: { $0.label == "single_enemy.ron" }) else {
                fatalError("Live check requires the repository scene catalog")
            }
            let started = await model.send(Request(op: .startScene, path: scene.path))
            precondition(started && !model.failed)
            let runBeforeRestart = model.state!.scenes.run
            let field = model.state!.fields.first!
            let original = field.value
            model.drafts[field.key] = "6"
            await model.submit(field)
            precondition(model.state!.fields.first!.value == 6)
            await model.action(.save)
            precondition(!model.state!.unsaved)
            model.drafts[field.key] = "-1"
            await model.submit(field)
            precondition(model.failed && model.state!.fields.first!.value == 6)
            await model.action(.defaults)
            precondition(model.state!.fields.first!.value == field.default)
            await model.action(.revert)
            precondition(model.state!.fields.first!.value == 6)
            await model.action(.restart)
            precondition(!model.failed && model.state!.scenes.run == runBeforeRestart + 1)
            precondition(model.state!.fields.first!.value == 6)
            await model.set(field, value: original)
            await model.action(.save)
            print("Native companion: live game edit, refusal, save, revert, defaults and restart passed")
            try await liveToolingChecks(model)
            try await liveSceneEditorChecks(model)
        }
    }
}

private func sceneDocument(_ name: String, path: String? = nil, revision: String? = nil, objectCount: Int = 0) throws -> SceneDocument {
    let objects: [[String: Any]] = (0..<objectCount).map { index in
        ["id": "body:\(index)", "kind": "body", "index": index, "label": "Enemy \(index+1)", "name": "", "role": "Enemy",
         "x": index, "z": 0, "width": 0, "depth": 0, "yaw": 0, "radius": 0.3, "fields": []]
    }
    let data = try JSONSerialization.data(withJSONObject: [
        "document": ["engine": ["name": name], "future_relationship": ["body": 3]],
        "name": name, "objects": objects, "selected": NSNull(), "path": path as Any? ?? NSNull(),
        "revision": revision as Any? ?? NSNull(), "arena_half": 96, "player_radius": 0.3,
        "projection": [[0.8,-0.6],[-0.3,-0.4]], "lighting": [], "encounters": [], "rewards": []
    ])
    return try JSONDecoder().decode(SceneDocument.self, from: data)
}

@MainActor
func sceneWideEditsPreserveSelection() throws {
    let model = Model(path: "unused")
    model.acceptEditor(try sceneDocument("Original", objectCount: 2), request: EditorRequest(action: "open"))
    model.editorSelection = "body:1"
    model.acceptEditor(try sceneDocument("Renamed", objectCount: 2), request: EditorRequest(action: "edit"))
    precondition(model.editorSelection == "body:1", "Scene-wide edits must retain an existing object selection")
    model.acceptEditor(try sceneDocument("Renamed", objectCount: 1), request: EditorRequest(action: "edit"))
    precondition(model.editorSelection == nil, "Deleted objects cannot leave a stale selection")
    model.acceptEditor(try sceneDocument("Original", objectCount: 2), request: EditorRequest(action: "open"))
    model.editorSelection = "body:0"
    model.acceptEditor(try sceneDocument("After deletion", objectCount: 1),
        request: EditorRequest(action: "edit", edit: .object(["action": .string("delete")])))
    precondition(model.editorSelection == nil, "A successor reusing an index must not inherit selection or inspector drafts")
}

@MainActor
func sceneDraftsSurviveDisconnectAndUndoAcrossSave() async throws {
    let peer = Peer()
    let model = Model(path: "unused", exchange: { try await peer.exchange($0) },
        author: { _ in AuthoringReply(ok: true, error: nil, editor: try sceneDocument("Offline edit")) })
    await model.send(Request(op: .state))
    let first = try sceneDocument("Original", path: "/scenes/a.ron", revision: "one")
    model.acceptEditor(first, request: EditorRequest(action: "open"))
    precondition(!model.editorDirty)
    let changed = try sceneDocument("Changed")
    model.acceptEditor(changed, request: EditorRequest(action: "edit"))
    precondition(model.editorDirty && model.editorPath == "/scenes/a.ron")
    await peer.disconnect()
    await model.send(Request(op: .state))
    precondition(!model.connected && model.editor?.document == changed.document)
    let accepted = await model.editScene(.object(["action": .string("rename"), "name": .string("Offline edit")]))
    precondition(accepted && model.editor?.name == "Offline edit")
    model.undoScene(); precondition(model.editor?.document == changed.document)
    await peer.restart()
    await model.send(Request(op: .state))
    precondition(model.connected && model.editor?.document == changed.document)
    model.acceptEditor(try sceneDocument("Changed", path: "/scenes/a.ron", revision: "two"), request: EditorRequest(action: "save"))
    precondition(!model.editorDirty)
    model.undoScene()
    precondition(model.editor?.document == first.document && model.editorDirty)
    precondition(model.editorRevision == "two", "Undo must not revert the disk revision")
    model.redoScene()
    precondition(model.editor?.document == changed.document && !model.editorDirty)
    let encoded = try JSONEncoder().encode(changed.document)
    let decoded = try JSONDecoder().decode(JSONValue.self, from: encoded)
    precondition(decoded == changed.document)
}

func sceneJSONPreservesExactIntegers() throws {
    let source = "[9007199254740993,18446744073709551615,-9223372036854775808,1.25,true]".data(using: .utf8)!
    let value = try JSONDecoder().decode(JSONValue.self, from: source)
    precondition(value == .array([.unsigned(9_007_199_254_740_993), .unsigned(UInt64.max), .signed(Int64.min), .number(1.25), .bool(true)]))
    let encoded = try JSONEncoder().encode(value)
    precondition(encoded == source, "Unedited authored values must survive the native editor exactly")
}

func sceneCoordinatesRoundTrip() {
    let view = SceneViewport(centreX: 7, centreZ: -3, span: 48)
    // Deliberately different from today's camera: the client must follow the
    // supplied projection, with no embedded isometric angle assumptions.
    let projection = SceneProjection([[0.8,-0.6],[-0.3,-0.4]])
    for invalid in ["[]", "[[1,0]]", "[[1,0],[2,0]]"] {
        precondition((try? JSONDecoder().decode(SceneProjection.self, from: Data(invalid.utf8))) == nil,
            "Malformed or singular camera matrices must not reach pointer projection")
    }
    for extent in [0.1, 48, 384, 2_000_000, 10_000_000] {
        precondition(extent / SceneViewport.gridSpacing(extent: extent) <= 80)
    }
    for size in [(400.0, 300.0), (700.0, 800.0)] {
        let (a,b) = view.screen(x: 4, z: 8, width: size.0, height: size.1, projection: projection)
        let (x,z) = view.world(x: a, y: b, width: size.0, height: size.1, snap: 0, projection: projection)
        precondition(abs(x-4)<0.000001 && abs(z-8)<0.000001)
        let (c,d) = view.screen(x: 4.2, z: -8.3, width: size.0, height: size.1, projection: projection)
        let snapped = view.world(x: c, y: d, width: size.0, height: size.1, snap: 0.5, projection: projection)
        precondition(snapped.0 == 4 && snapped.1 == -8.5)
        let (dx,dz) = view.delta(x: 25, y: -19, width: size.0, height: size.1, projection: projection)
        let shifted = view.screen(x: 4+dx, z: 8+dz, width: size.0, height: size.1, projection: projection)
        precondition(abs(shifted.0-a-25)<0.000001 && abs(shifted.1-b+19)<0.000001, "Dragging follows screen-space pointer motion")
    }
}

@MainActor
func liveSceneEditorChecks(_ model: Model) async throws {
    let run = model.state?.scenes.run
    let created = await model.sendAuthoring(EditorRequest(action: "new"))
    precondition(created)
    await model.editScene(.object(["action": .string("add"), "kind": .string("source"), "pos": .point(8, 8)]))
    precondition(!model.editorFailed && model.editor!.objects.count == 1)
    let source = model.editor!.objects[0]
    await model.editScene(.object(["action": .string("duplicate"), "target": source.target, "offset": .point(3, 0)]))
    precondition(!model.editorFailed && model.editor!.objects.count == 2)
    model.undoScene(); precondition(model.editor!.objects.count == 1)
    model.redoScene(); precondition(model.editor!.objects.count == 2)
    await model.editScene(.object(["action": .string("set"), "target": source.target, "field": .string("radius"), "value": .number(-2)]))
    precondition(model.editorFailed && model.editor!.objects[0].radius == 2)
    precondition(model.state?.scenes.run == run)
    await model.editScene(.object(["action": .string("add"), "kind": .string("tree"), "pos": .point(-8, 8)]))
    precondition(!model.editorFailed)
    let tree = model.editor!.objects.first { $0.kind == "tree" }!
    for (field, value) in [("kind", 2.0), ("yaw", 30.0), ("scale", 1.5)] {
        await model.editScene(.object(["action": .string("set"), "target": tree.target, "field": .string(field), "value": .number(value)]))
        precondition(!model.editorFailed)
    }
    await model.editScene(.object(["action": .string("duplicate"), "target": tree.target, "offset": .point(3, 0)]))
    await model.editScene(.object(["action": .string("move"), "target": tree.target, "pos": .point(-10, 7)]))
    precondition(!model.editorFailed && model.editor!.objects.filter { $0.kind == "tree" }.count == 2)
    await model.editScene(.object(["action": .string("add"), "kind": .string("campfire"), "pos": .point(4, 6)]))
    precondition(!model.editorFailed)
    let campfire = model.editor!.objects.first { $0.kind == "campfire" }!
    await model.editScene(.object(["action": .string("set"), "target": campfire.target, "field": .string("range"), "value": .number(9)]))
    await model.editScene(.object(["action": .string("colour"), "target": campfire.target, "colour": .array([.number(0.5),.number(0.25),.number(0.125)])]))
    precondition(!model.editorFailed)
    await model.editScene(.object(["action": .string("lighting"), "field": .string("ambient_intensity"), "value": .number(0.25)]))
    await model.editScene(.object(["action": .string("lighting_colour"), "field": .string("ambient_colour"), "colour": .array([.number(0.25),.number(0.5),.number(1)])]))
    precondition(!model.editorFailed)
    for (kind, x) in [("switch", -3.0), ("door", 3.0), ("chest", 6.0), ("door", 9.0)] {
        await model.editScene(.object(["action": .string("place"), "kind": .string(kind), "pos": .point(x, -4)]))
        precondition(!model.editorFailed)
    }
    let trigger = model.editor!.objects.first { $0.role == "Switch" }!
    await model.editScene(.object(["action": .string("name"), "target": trigger.target, "name": .string("Trial shrine")]))
    precondition(!model.editorFailed)
    var form = EncounterForm(document: model.editor!, encounter: nil)
    form.name = "Editor trial"; form.sources = [0, 1]; form.count = "4"; form.ticks = "1200"
    let rewards = model.editor!.rewards
    precondition(rewards.count == 3)
    form.onStart = [rewards[0].action]; form.onSuccess = [rewards[2].action]; form.onFailure = [rewards[1].action]
    await model.editScene(form.edit!)
    precondition(!model.editorFailed && model.editor!.encounters.count == 1)
    precondition(model.editor!.objects.filter { $0.kind == "source" }.allSatisfy { $0.value("enabled") == 0 })
    let accepted = model.editor!.document
    await model.editScene(.object(["action": .string("delete"), "target": trigger.target]))
    precondition(model.editorFailed && model.editor!.document == accepted)
    model.undoScene(); precondition(model.editor!.encounters.isEmpty)
    model.redoScene(); precondition(model.editor!.encounters.count == 1)
    let authored = model.editor!.document
    let path = FileManager.default.temporaryDirectory.appendingPathComponent("arpg-edited-\(UUID().uuidString).ron").path
    defer { try? FileManager.default.removeItem(atPath: path) }
    await model.saveScene(path: path, play: model.connected)
    precondition(!model.editorFailed && !model.editorDirty)
    if let run { precondition(model.state?.scenes.run == run + 1) }
    let reopened = await model.sendAuthoring(EditorRequest(action: "open", path: path))
    precondition(reopened)
    precondition(model.editor!.objects.count == 9 && model.editor!.document == authored)
    precondition(model.editor!.lighting.first { $0.key == "ambient_intensity" }!.value == 0.25)
    precondition(model.editor!.objects.first { $0.kind == "campfire" }!.radius == 9)
    let agent = Authoring()
    let agentRead = try await agent.exchange(EditorRequest(action: "open", path: path))
    precondition(agentRead.ok && agentRead.editor?.document == authored)
    print("Native companion: shared headless scene props, lighting, named encounter, reward actions, undo/redo, refusal, save/play and reopen passed")
}

func encounterFormsPreserveCompleteRelationships() throws {
    let action = JSONValue.object(["unlock_door": .unsigned(1)])
    let encounter = SceneEncounter(index: 2, name: "Trial", trigger: 3, sources: [2, 0], count: 30,
        within_ticks: 1800, on_start: [action], on_success: [], on_failure: [action])
    let rewards = [SceneReward(body: 7, label: "Exit", action: action)]
    precondition(encounter.objectIDs(rewards: rewards) == ["body:3", "source:2", "source:0", "body:7"])
    var form = EncounterForm(document: try sceneDocument("Trial"), encounter: encounter)
    precondition(form.sources == [2, 0] && form.onStart == [action] && form.onFailure == [action])
    form.count = String(UInt32.max); form.ticks = String(UInt32.max)
    guard case .object(let command) = form.edit, case .object(let value) = command["value"] else {
        fatalError("Complete form must generate one transaction")
    }
    precondition(command["index"] == .number(2))
    precondition(value["count"] == .unsigned(UInt64(UInt32.max)) && value["within_ticks"] == .unsigned(UInt64(UInt32.max)))
    precondition(value["on_start"] == .array([action]) && value["on_failure"] == .array([action]))
    for invalid in ["0", "-1", "1.5", "4294967296", ""] {
        form.count = invalid; precondition(form.edit == nil)
    }
    form.count = "1"; form.sources = []; precondition(form.edit == nil)
    form.sources = [0]; form.trigger = nil; precondition(form.edit == nil)
}

func integerInspectorPreservesPrecision() throws {
    for n in [UInt64(16_777_217), UInt64(UInt32.max), UInt64.max] {
        let data = Data("{\"key\":\"threshold\",\"label\":\"Population\",\"unit\":\"enemies\",\"value\":\(n),\"step\":1,\"integer\":true}".utf8)
        let field = try JSONDecoder().decode(SceneField.self, from: data)
        precondition(field.integerValue == n)
        precondition(field.parsed(String(n)) == .unsigned(n))
        precondition(field.stepped(up: false) == .unsigned(n - 1))
        precondition(field.stepped(up: true) == (n == UInt64.max ? nil : .unsigned(n + 1)))
        for invalid in ["-1", "1.5", "18446744073709551616", ""] { precondition(field.parsed(invalid) == nil) }
    }
}
