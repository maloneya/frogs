import Foundation

// A synthetic catalog intentionally uses an unknown field: UI must not depend on
// the current game's seven keys, groups, ranges or defaults.
private func reply(value: Double = 9, session: UInt32 = 1, ok: Bool = true) throws -> Reply {
    let data = """
    {"ok":\(ok),"error":\(ok ? "null" : "\"Out of range\""),"state":{
    "session":\(session),"fields":[{"key":"future_field","group":"Future type","label":"Speed","unit":"m/s","default":9,"min":0,"max":60,"step":0.1,"value":\(value),"saved":9}],
    "unsaved":\(value != 9),"path":"tuning.ron","message":"Ready","error":false,"busy":false}}
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


@main
struct Checks {
    @MainActor static func main() async throws {
        try await draftsSurviveRefreshButNeverReconnect()
        try await invalidTextNeverLeavesTheCompanion()
        await localTransportFailsClearlyWithoutAGame()
        await editAdmissionWaitsForRefreshWithoutQueuingMoreEdits()
        print("Native companion: draft, validation, reconnect and transport checks passed")
        if let path = ProcessInfo.processInfo.environment["ARPG_TUNING_TEST_SOCKET"] {
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
        }
    }
}
