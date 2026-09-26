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
    await model.action(.defaultScene)
    precondition(!model.failed && model.state!.scenes.run == started.run + 2)
    print("Native companion: live scene start/refusal/restart, catalog, collision, vsync, zoom and screenshot success/failure passed")
}


@main
struct Checks {
    @MainActor static func main() async throws {
        try await draftsSurviveRefreshButNeverReconnect()
        try await invalidTextNeverLeavesTheCompanion()
        await localTransportFailsClearlyWithoutAGame()
        await editAdmissionWaitsForRefreshWithoutQueuingMoreEdits()
        print("Native companion: draft, validation, reconnect and transport checks passed")
        if let path = ProcessInfo.processInfo.environment["ARPG_COMPANION_TEST_SOCKET"] {
            let model = Model(path: path)
            await model.send(Request(op: .state))
            precondition(model.connected)
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
            precondition(model.state!.fields.first!.value == 6)
            await model.set(field, value: original)
            await model.action(.save)
            print("Native companion: live game edit, refusal, save, revert, defaults and restart passed")
            try await liveToolingChecks(model)
        }
    }
}
