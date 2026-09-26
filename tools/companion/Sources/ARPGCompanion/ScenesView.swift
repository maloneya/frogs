import SwiftUI
import AppKit
import UniformTypeIdentifiers

struct ScenesView: View {
    @ObservedObject var model: Model
    @Binding var selection: String?
    @State private var tool = "select"
    @State private var snap = 0.5
    @State private var viewport = SceneViewport()
    @State private var pendingOpen: EditorRequest?
    @State private var confirmDiscard = false
    @State private var fieldDrafts: [String: String] = [:]
    @State private var fieldError: String?
    @State private var nameDraft = ""

    private var selected: SceneObject? { model.editor?.objects.first { $0.id == model.editorSelection } }
    var body: some View {
        VStack(spacing: 0) {
            HStack {
                Text(model.editor.map { $0.name + (model.editorDirty ? " *" : "") } ?? "Scene editor").font(.title2.bold()).lineLimit(1)
                Spacer()
                Button("Restart current") { Task { await model.action(.restart) } }.disabled(!model.editable)
                Button("Default horde") { Task { await model.action(.defaultScene) } }.disabled(!model.editable)
            }.padding(12)
            HSplitView {
                VStack(alignment: .leading, spacing: 10) {
                    HStack {
                        Text("Scene files").font(.headline)
                        Spacer()
                        Button { Task { await model.action(.refreshScenes) } } label: { Image(systemName: "arrow.clockwise") }
                            .help("Refresh scene files").disabled(!model.editable)
                    }
                    List(model.state?.scenes.files ?? [], selection: $selection) { file in
                        Text(file.label).lineLimit(1).help(file.path).tag(file.path)
                    }.listStyle(.plain).scrollContentBackground(.hidden)
                    Button("Open selected") {
                        if let selection { open(EditorRequest(action: "open", path: selection)) }
                    }.disabled(!model.editable || selection == nil)
                    Button("New scene") { open(EditorRequest(action: "new")) }.disabled(!model.editable)
                    if let error = model.state?.scenes.error { Text(error).font(.caption).foregroundStyle(.red) }
                }.padding(12).frame(minWidth: 150, idealWidth: 180, maxWidth: 220)
                if let document = model.editor {
                    VStack(spacing: 0) {
                        toolbar
                            .onAppear { nameDraft = document.name; fit(document) }
                        SceneCanvas(document: document, selection: $model.editorSelection, viewport: $viewport,
                                    tool: tool, snap: snap, editable: model.editable, edit: { edit in Task { await model.editScene(edit) } })
                        HStack {
                            Text("X →  Z ↑ · metres · P = player start").font(.caption).foregroundStyle(.secondary)
                            Spacer()
                            Button("Fit") { fit(document) }
                            Button { viewport.span = min(384, viewport.span * 1.4) } label: { Image(systemName: "minus.magnifyingglass") }.help("Zoom out")
                            Button { viewport.span = max(4, viewport.span / 1.4) } label: { Image(systemName: "plus.magnifyingglass") }.help("Zoom in")
                        }.padding(10)
                    }.frame(minWidth: 340)
                    inspector(document).frame(minWidth: 210, idealWidth: 240, maxWidth: 280)
                } else {
                    ContentUnavailableView("Open a scene to edit", systemImage: "square.grid.3x3", description: Text("Place enemies, draw walls and arrange sources on a map. Save & play starts the layout in the game."))
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                }
            }
            Divider()
            HStack {
                VStack(alignment: .leading, spacing: 3) {
                    Text(model.editorPath ?? "New scene · choose a file when saving").lineLimit(1).truncationMode(.middle)
                    Text(fieldError ?? ((!fieldDrafts.isEmpty || nameDraft != model.editor?.name) ? "Press Return to apply inspector edits, or Escape to discard them." : "Editing the starting layout. The running game stays unchanged until Save & play."))
                        .foregroundStyle(fieldError == nil ? Color.secondary : .red).lineLimit(2)
                }.font(.caption)
                Spacer()
                Button("Save as…") { save(asNew: true, play: false) }.disabled(!canSave)
                Button("Save") { save(asNew: false, play: false) }.keyboardShortcut("s").disabled(!canSave)
                Button("Save & play") { save(asNew: false, play: true) }.buttonStyle(.borderedProminent).disabled(!canSave)
            }.padding(12)
        }
        .confirmationDialog("Discard this scene draft?", isPresented: $confirmDiscard) {
            Button("Discard and open", role: .destructive) { if let pendingOpen { executeOpen(pendingOpen) } }
            Button("Cancel", role: .cancel) { pendingOpen = nil }
        } message: { Text("Save the current scene first if you want to keep it.") }
        .onChange(of: model.editorSelection) { _, _ in fieldDrafts.removeAll(); fieldError = nil }
    }
    private var canSave: Bool { model.editable && model.editor != nil && fieldDrafts.isEmpty && nameDraft == model.editor?.name }
    private var toolbar: some View {
        VStack(alignment: .leading, spacing: 8) {
            Picker("Tool", selection: $tool) {
                Text("Select").tag("select"); Text("Pan").tag("pan"); Text("Enemy").tag("body")
                Text("Wall").tag("wall"); Text("Source").tag("source"); Text("Formation").tag("grid")
            }.pickerStyle(.segmented).labelsHidden()
            HStack {
                Text(tool == "wall" ? "Drag to draw a wall" : tool == "select" ? "Click to select · drag to move" : tool == "pan" ? "Drag to pan the map" : "Click the map to place").font(.caption).foregroundStyle(.secondary)
                Spacer()
                Picker("Snap", selection: $snap) {
                    Text("Off").tag(0.0); Text("0.25 m").tag(0.25); Text("0.5 m").tag(0.5); Text("1 m").tag(1.0)
                }.fixedSize()
            }
        }.padding(10)
    }
    private func inspector(_ document: SceneDocument) -> some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 14) {
                Text("Scene name").font(.headline)
                TextField(document.name, text: $nameDraft).textFieldStyle(.roundedBorder)
                    .onAppear { nameDraft = document.name }
                    .onChange(of: document.name) { _, value in nameDraft = value }
                    .onSubmit { Task { await model.editScene(.object(["action": .string("rename"), "name": .string(nameDraft)])) } }
                    .onExitCommand { nameDraft = document.name }
                    .disabled(!model.editable)
                Divider()
                Picker("Object", selection: $model.editorSelection) {
                    Text("No selection").tag(String?.none)
                    ForEach(document.objects) { object in Text(object.label).tag(Optional(object.id)) }
                }
                if let object = selected {
                    Text(object.label).font(.headline)
                    ForEach(object.fields) { field in inspectorField(object, field) }
                    HStack {
                        Button("Duplicate") { action("duplicate", object) }.keyboardShortcut("d").disabled(!model.editable)
                        Button("Delete", role: .destructive) { action("delete", object) }.disabled(!model.editable)
                    }
                    Text("Duplicate copies the placement; encounter links stay with the original.").font(.caption).foregroundStyle(.secondary)
                }
                Divider()
                HStack {
                    Button("Undo") { model.undoScene(); fieldDrafts.removeAll() }.keyboardShortcut("z").disabled(model.busy || model.undoScenes.isEmpty)
                    Button("Redo") { model.redoScene(); fieldDrafts.removeAll() }.keyboardShortcut("z", modifiers: [.command, .shift]).disabled(model.busy || model.redoScenes.isEmpty)
                }
                if document.relationships > 0 || !document.trees.isEmpty || !document.torches.isEmpty {
                    Text("Encounter links, trees and torches are preserved. Trees and torches appear as reference markers.").font(.caption).foregroundStyle(.secondary)
                }
            }.padding(14)
        }
    }
    @ViewBuilder private func inspectorField(_ object: SceneObject, _ field: SceneField) -> some View {
        if field.unit == "bool" {
            Toggle(field.label, isOn: Binding(get: { field.value != 0 }, set: { set(object, field, $0 ? 1 : 0) })).disabled(!model.editable)
        } else if let options = field.options {
            Picker(field.label, selection: Binding(get: { Int(field.value) }, set: { set(object, field, Double($0)) })) {
                ForEach(Array(options.enumerated()), id: \.offset) { i, label in Text(label).tag(i) }
            }.disabled(!model.editable)
        } else {
            VStack(alignment: .leading, spacing: 4) {
                Text(field.label + (field.unit.isEmpty ? "" : " · \(field.unit)")).font(.caption)
                HStack {
                    TextField(field.label, text: Binding(get: { fieldDrafts[field.key] ?? number(field.value) }, set: { fieldDrafts[field.key] = $0 }))
                        .textFieldStyle(.roundedBorder).onSubmit {
                            guard let value = Double(fieldDrafts[field.key] ?? ""), value.isFinite else { fieldError = "Enter a finite number."; return }
                            set(object, field, value)
                        }.onExitCommand { fieldDrafts.removeValue(forKey: field.key); fieldError = nil }
                    Stepper(field.label, onIncrement: { set(object, field, field.value + field.step) }, onDecrement: { set(object, field, field.value - field.step) }).labelsHidden().fixedSize()
                }.disabled(!model.editable)
            }
        }
    }
    private func set(_ object: SceneObject, _ field: SceneField, _ value: Double) {
        let submittedDraft = fieldDrafts[field.key]
        Task {
            if await model.editScene(.object(["action": .string("set"), "target": object.target, "field": .string(field.key), "value": .number(value)])),
               model.editorSelection == object.id, fieldDrafts[field.key] == submittedDraft {
                fieldDrafts.removeValue(forKey: field.key); fieldError = nil
            }
        }
    }
    private func action(_ action: String, _ object: SceneObject) {
        var edit: [String: JSONValue] = ["action": .string(action), "target": object.target]
        if action == "duplicate" { edit["offset"] = .point(max(snap, 1), -max(snap, 1)) }
        Task { await model.editScene(.object(edit)) }
    }
    private func open(_ request: EditorRequest) {
        pendingOpen = request
        if model.editorDirty || !fieldDrafts.isEmpty || (model.editor != nil && nameDraft != model.editor?.name) { confirmDiscard = true } else { executeOpen(request) }
    }
    private func executeOpen(_ request: EditorRequest) {
        Task {
            if await model.send(Request(op: .sceneEditor, request: request)), let document = model.editor {
                fieldDrafts.removeAll(); fieldError = nil; nameDraft = document.name; fit(document)
            }
        }
    }
    private func save(asNew: Bool, play: Bool) {
        if !asNew, let path = model.editorPath { Task { await model.saveScene(path: path, play: play) }; return }
        let panel = NSSavePanel()
        panel.title = play ? "Save scene and play" : "Save scene"
        panel.nameFieldStringValue = "encounter.ron"
        panel.allowedContentTypes = [UTType(filenameExtension: "ron") ?? .data]
        if let directory = model.state?.scenes.directory { panel.directoryURL = URL(fileURLWithPath: directory) }
        panel.begin { result in
            if result == .OK, let url = panel.url { Task { await model.saveScene(path: url.path, play: play) } }
        }
    }
    private func fit(_ document: SceneDocument) {
        var xs: [Double] = [0]
        var zs: [Double] = [0]
        for o in document.objects {
            let extent = max(o.width, o.depth) + o.radius
            xs += [o.x - extent, o.x + extent]
            zs += [o.z - extent, o.z + extent]
        }
        for o in document.trees + document.torches { xs.append(o.x); zs.append(o.z) }
        viewport.centreX = max(-document.arena_half, min(document.arena_half, (xs.min()! + xs.max()!) / 2))
        viewport.centreZ = max(-document.arena_half, min(document.arena_half, (zs.min()! + zs.max()!) / 2))
        viewport.span = min(384, max(16, max(xs.max()! - xs.min()!, zs.max()! - zs.min()!) * 1.3))
    }
}
