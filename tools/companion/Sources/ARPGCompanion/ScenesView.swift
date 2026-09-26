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
    @State private var colourDrafts: [String: Color] = [:]
    @State private var fieldError: String?
    @State private var nameDraft = ""

    private var selected: SceneObject? { model.editor?.objects.first { $0.id == model.editorSelection } }
    var body: some View {
        VStack(spacing: 0) {
            HStack {
                Text(model.editor.map { $0.name + (model.editorDirty ? " *" : "") } ?? "Scene editor").font(.title2.bold()).lineLimit(1)
                Spacer()
                Text(model.state?.scenes.current.map { "Playing: \($0)" } ?? "Choose a scene to play")
                    .font(.caption).foregroundStyle(.secondary).lineLimit(1)
                Button("Restart scene") { Task { await model.action(.restart) } }.disabled(!model.editable || model.state?.scenes.current == nil)
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
                            Text("Game camera orientation · metres · P = player start").font(.caption).foregroundStyle(.secondary)
                            Spacer()
                            Button("Fit") { fit(document) }
                            Button { viewport.span *= 1.4 } label: { Image(systemName: "minus.magnifyingglass") }.help("Zoom out")
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
                    Text(fieldError ?? (hasPendingInspectorEdits ? "Apply colour changes; press Return for numeric edits or Escape to discard text." : "Editing the starting layout. The running game stays unchanged until Save & play."))
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
        .onChange(of: model.editorSelection) { _, _ in
            let sceneKeys = Set(model.editor?.lighting.map(\.key) ?? [])
            fieldDrafts = fieldDrafts.filter { sceneKeys.contains($0.key) }
            colourDrafts = colourDrafts.filter { sceneKeys.contains($0.key) }
            fieldError = nil
        }
    }
    private var hasPendingInspectorEdits: Bool { !fieldDrafts.isEmpty || !colourDrafts.isEmpty || (model.editor != nil && nameDraft != model.editor?.name) }
    private var canSave: Bool { model.editable && model.editor != nil && !hasPendingInspectorEdits }
    private var toolbar: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                Picker("Tool", selection: $tool) {
                    Label("Select", systemImage: "cursorarrow").tag("select")
                    Label("Pan", systemImage: "hand.draw").tag("pan")
                    Divider()
                    Label("Enemy", systemImage: "figure.stand").tag("body")
                    Label("Wall", systemImage: "rectangle").tag("wall")
                    Label("Source", systemImage: "circle.dashed").tag("source")
                    Label("Formation", systemImage: "square.grid.3x3").tag("grid")
                    Label("Tree", systemImage: "tree").tag("tree")
                    Label("Campfire", systemImage: "flame").tag("campfire")
                }.pickerStyle(.menu).frame(maxWidth: 200)
                Spacer()
                Button("Select") { tool = "select" }.help("Return to selecting and moving objects")
            }
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
                DisclosureGroup("Scene lighting") {
                    VStack(alignment: .leading, spacing: 12) {
                        ForEach(document.lighting) { field in inspectorField(nil, field) }
                        Text("Saved with the scene. Save & play applies these settings.").font(.caption).foregroundStyle(.secondary)
                    }.padding(.top, 8)
                }
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
                    Button("Undo") { model.undoScene(); fieldDrafts.removeAll(); colourDrafts.removeAll() }.keyboardShortcut("z").disabled(model.busy || model.undoScenes.isEmpty)
                    Button("Redo") { model.redoScene(); fieldDrafts.removeAll(); colourDrafts.removeAll() }.keyboardShortcut("z", modifiers: [.command, .shift]).disabled(model.busy || model.redoScenes.isEmpty)
                }
                if document.relationships > 0 {
                    Text("Encounter links are preserved. Linked objects cannot be deleted while their relationships need them.").font(.caption).foregroundStyle(.secondary)
                }
            }.padding(14)
        }
    }
    @ViewBuilder private func inspectorField(_ object: SceneObject?, _ field: SceneField) -> some View {
        if let rgb = field.colour, rgb.count == 3 {
            VStack(alignment: .leading, spacing: 4) {
                ColorPicker(field.label, selection: Binding(get: { colourDrafts[field.key] ?? Color(.sRGBLinear, red: rgb[0], green: rgb[1], blue: rgb[2]) },
                    set: { colourDrafts[field.key] = $0 }), supportsOpacity: false).disabled(!model.editable)
                if let draft = colourDrafts[field.key] {
                    HStack {
                        Button("Apply colour") { setColour(object, field, draft) }.disabled(!model.editable)
                        Button("Discard") { colourDrafts.removeValue(forKey: field.key) }
                    }.controlSize(.small)
                }
            }
        } else if field.unit == "bool" {
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
    private func set(_ object: SceneObject?, _ field: SceneField, _ value: Double) {
        let submittedDraft = fieldDrafts[field.key]
        var edit: [String: JSONValue] = ["action": .string(object == nil ? "lighting" : "set"), "field": .string(field.key), "value": .number(value)]
        if let object { edit["target"] = object.target }
        Task {
            if await model.editScene(.object(edit)),
               (object == nil || model.editorSelection == object?.id), fieldDrafts[field.key] == submittedDraft {
                fieldDrafts.removeValue(forKey: field.key); fieldError = nil
            }
        }
    }
    private func setColour(_ object: SceneObject?, _ field: SceneField, _ colour: Color) {
        guard let space = CGColorSpace(name: CGColorSpace.linearSRGB),
              let components = NSColor(colour).cgColor.converted(to: space, intent: .defaultIntent, options: nil)?.components,
              components.count >= 3 else { return }
        let rgb = Array(components.prefix(3)).map { JSONValue.number(min(1, max(0, Double($0)))) }
        var edit: [String: JSONValue] = ["action": .string(object == nil ? "lighting_colour" : "colour"), "colour": .array(rgb)]
        if let object { edit["target"] = object.target } else { edit["field"] = .string(field.key) }
        Task {
            if await model.editScene(.object(edit)), colourDrafts[field.key] == colour,
               object == nil || model.editorSelection == object?.id {
                colourDrafts.removeValue(forKey: field.key)
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
        if model.editorDirty || hasPendingInspectorEdits { confirmDiscard = true } else { executeOpen(request) }
    }
    private func executeOpen(_ request: EditorRequest) {
        Task {
            if await model.send(Request(op: .sceneEditor, request: request)), let document = model.editor {
                fieldDrafts.removeAll(); colourDrafts.removeAll(); fieldError = nil; nameDraft = document.name; fit(document)
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
        viewport.fit(document)
    }
}
