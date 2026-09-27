import SwiftUI
import AppKit
import UniformTypeIdentifiers

struct ScenesView: View {
    @ObservedObject var model: Model
    @State private var tool = "select"
    @State private var snap = 0.5
    @State private var viewport = SceneViewport()
    @State private var pendingOpen: EditorRequest?
    @State private var confirmDiscard = false
    @State private var fieldDrafts: [String: String] = [:]
    @State private var colourDrafts: [String: Color] = [:]
    @State private var fieldError: String?
    @State private var nameDraft = ""
    @State private var objectNameDraft = ""
    @State private var encounterForm: EncounterForm?

    private var selected: SceneObject? { model.editor?.objects.first { $0.id == model.editorSelection } }
    var body: some View {
        VStack(spacing: 0) {
            HStack {
                Text(model.editor.map { $0.name + (model.editorDirty ? " *" : "") } ?? "Scene editor").font(.title2.bold()).lineLimit(1)
                sceneFilesMenu
                Spacer()
                Text(model.state?.scenes.current.map { "Playing: \($0)" } ?? "Choose a scene to play")
                    .font(.caption).foregroundStyle(.secondary).lineLimit(1)
                Button("Restart scene") { Task { await model.action(.restart) } }.disabled(!model.editable || model.state?.scenes.current == nil)
            }.padding(12)
            HSplitView {
                toolList
                if let document = model.editor {
                    VStack(spacing: 0) {
                        toolbar
                            .onAppear { nameDraft = document.name; fit(document) }
                        SceneCanvas(document: document, selection: $model.editorSelection, viewport: $viewport,
                                    encounter: document.encounters.first { $0.index == model.editorEncounter },
                                    tool: tool, snap: snap, editable: model.canAuthor, deleteSelection: deleteSelection, edit: { edit in Task { await model.editScene(edit) } })
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
                    ContentUnavailableView("Open a scene to edit", systemImage: "square.grid.3x3", description: Text("Place actors, draw walls and arrange sources on a map. Save & play starts the layout in the game."))
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                }
            }
            Divider()
            HStack {
                VStack(alignment: .leading, spacing: 3) {
                    Text(model.editorPath ?? "New scene · choose a file when saving").lineLimit(1).truncationMode(.middle)
                    Text(fieldError ?? (model.editorFailed ? model.editorNotice : nil) ?? (hasPendingInspectorEdits ? "Apply colour changes; press Return for numeric edits or Escape to discard text." : model.editorNotice))
                        .foregroundStyle(fieldError == nil ? Color.secondary : .red).lineLimit(2)
                }.font(.caption)
                Spacer()
                Button("Save as…") { save(asNew: true, play: false) }.disabled(!canSave)
                Button("Save") { save(asNew: false, play: false) }.keyboardShortcut("s").disabled(!canSave)
                Button("Save & play") { save(asNew: false, play: true) }.buttonStyle(.borderedProminent).disabled(!canSave || !model.editable)
            }.padding(12)
        }
        .confirmationDialog("Discard this scene draft?", isPresented: $confirmDiscard) {
            Button("Discard and open", role: .destructive) { if let pendingOpen { executeOpen(pendingOpen) } }
            Button("Cancel", role: .cancel) { pendingOpen = nil }
        } message: { Text("Save the current scene first if you want to keep it.") }
        .sheet(item: $encounterForm) { form in
            if let document = model.editor {
                EncounterEditor(model: model, document: document, initial: form)
            }
        }
        .onChange(of: model.editorSelection) { _, _ in
            objectNameDraft = selected?.name ?? ""
            let sceneKeys = Set(model.editor?.lighting.map(\.key) ?? [])
            fieldDrafts = fieldDrafts.filter { sceneKeys.contains($0.key) }
            colourDrafts = colourDrafts.filter { sceneKeys.contains($0.key) }
            fieldError = nil
        }
    }
    private var hasPendingInspectorEdits: Bool { !fieldDrafts.isEmpty || !colourDrafts.isEmpty || (model.editor != nil && nameDraft != model.editor?.name) || (selected != nil && objectNameDraft != selected?.name) }
    private var canSave: Bool { model.canAuthor && model.editor != nil && !hasPendingInspectorEdits }
    private var sceneFilesMenu: some View {
        Menu("Scene files", systemImage: "folder") {
            Button("New scene") { open(EditorRequest(action: "new")) }
            Divider()
            ForEach(model.sceneFiles) { file in
                Button(file.label) { open(EditorRequest(action: "open", path: file.path)) }
                    .help(file.path)
            }
            if model.sceneFiles.isEmpty { Text("No scene files") }
            Divider()
            Button("Refresh files") { model.refreshSceneFiles() }
            if let error = model.sceneFileError { Text(error) }
        }.fixedSize().disabled(!model.canAuthor)
    }
    private var toolList: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 4) {
                Text("Tools").font(.headline).padding(.bottom, 8)
                toolButton("Select / Pan", "cursorarrow", "select")
                Divider().padding(.vertical, 6)
                toolButton("Actor", "figure.stand", "body")
                toolButton("Wall", "rectangle", "wall")
                toolButton("Source", "circle.dashed", "source")
                toolButton("Formation", "square.grid.3x3", "grid")
                toolButton("Tree", "tree", "tree")
                toolButton("Campfire", "flame", "campfire")
                Divider().padding(.vertical, 6)
                toolButton("Static prop", "cube", "prop")
                toolButton("Switch / shrine", "hand.tap", "switch")
                toolButton("Door", "door.left.hand.closed", "door")
                toolButton("Chest", "shippingbox", "chest")
            }.padding(12)
        }.frame(minWidth: 150, idealWidth: 180, maxWidth: 220)
            .disabled(model.editor == nil)
    }
    private func toolButton(_ label: String, _ symbol: String, _ value: String) -> some View {
        Button { tool = value } label: {
            Label(label, systemImage: symbol)
                .frame(maxWidth: .infinity, alignment: .leading).padding(8)
                .background(tool == value ? Color.accentColor.opacity(0.18) : .clear,
                            in: RoundedRectangle(cornerRadius: 6))
                .contentShape(Rectangle())
        }.buttonStyle(.plain).accessibilityAddTraits(tool == value ? [.isSelected] : [])
    }
    private var toolbar: some View {
        HStack {
            Text(tool == "select" ? "Either button: select / move · drag empty space to pan" :
                 tool == "wall" ? "Left drag: draw wall · right button: select / pan" :
                 "Left click: place · right button: select / pan")
                .font(.caption).foregroundStyle(.secondary)
            Spacer()
            Picker("Snap", selection: $snap) {
                Text("Off").tag(0.0); Text("0.25 m").tag(0.25); Text("0.5 m").tag(0.5); Text("1 m").tag(1.0)
            }.fixedSize()
        }.padding(10)
    }
    private func deleteSelection() {
        guard model.canAuthor, let selected else { return }
        action("delete", selected)
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
                    .disabled(!model.canAuthor)
                Divider()
                DisclosureGroup("Scene lighting") {
                    VStack(alignment: .leading, spacing: 12) {
                        ForEach(document.lighting) { field in inspectorField(nil, field) }
                        Text("Saved with the scene. Save & play applies these settings.").font(.caption).foregroundStyle(.secondary)
                    }.padding(.top, 8)
                }
                Divider()
                encounterControls(document)
                Divider()
                Picker("Object", selection: $model.editorSelection) {
                    Text("No selection").tag(String?.none)
                    ForEach(document.objects) { object in Text(object.label).tag(Optional(object.id)) }
                }
                if let object = selected {
                    Text(object.role).font(.headline)
                    TextField("Object name", text: $objectNameDraft)
                        .textFieldStyle(.roundedBorder)
                        .onAppear { objectNameDraft = object.name }
                        .onChange(of: object.name) { _, value in objectNameDraft = value }
                        .onExitCommand { objectNameDraft = object.name }
                        .onSubmit { Task { await model.editScene(.object(["action": .string("name"), "target": object.target, "name": .string(objectNameDraft)])) } }
                        .disabled(!model.canAuthor)
                    ForEach(object.fields) { field in inspectorField(object, field) }
                    HStack {
                        Button("Duplicate") { action("duplicate", object) }.keyboardShortcut("d").disabled(!model.canAuthor)
                        Button("Delete", role: .destructive, action: deleteSelection).disabled(!model.canAuthor)
                    }
                    Text("Duplicate copies the placement; encounter links stay with the original.").font(.caption).foregroundStyle(.secondary)
                }
                Divider()
                HStack {
                    Button("Undo") { model.undoScene(); fieldDrafts.removeAll(); colourDrafts.removeAll(); objectNameDraft = selected?.name ?? "" }.keyboardShortcut("z").disabled(model.busy || model.undoScenes.isEmpty)
                    Button("Redo") { model.redoScene(); fieldDrafts.removeAll(); colourDrafts.removeAll(); objectNameDraft = selected?.name ?? "" }.keyboardShortcut("z", modifiers: [.command, .shift]).disabled(model.busy || model.redoScenes.isEmpty)
                }
                Text("Actor definitions apply to a body, every formation member, or everything a source emits. Add actions, then combine them as a sequence or priority. Doors and chests start locked.").font(.caption).foregroundStyle(.secondary)
            }.padding(14)
        }
    }
    private func encounterControls(_ document: SceneDocument) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("Encounters").font(.headline)
            Picker("Show links", selection: $model.editorEncounter) {
                Text("None").tag(Int?.none)
                ForEach(document.encounters) { encounter in Text(encounter.name).tag(Optional(encounter.index)) }
            }
            Button("New encounter") { encounterForm = EncounterForm(document: document, encounter: nil) }.disabled(!model.canAuthor)
            if let encounter = document.encounters.first(where: { $0.index == model.editorEncounter }) {
                Text("Defeat \(encounter.count) within \(encounter.within_ticks) ticks").font(.caption)
                HStack {
                    Button("Edit") { encounterForm = EncounterForm(document: document, encounter: encounter) }.disabled(!model.canAuthor)
                    Button("Frame") { viewport.fit(document, objectIDs: encounter.objectIDs(rewards: document.rewards)) }
                    Button("Delete", role: .destructive) {
                        Task { await model.editScene(.object(["action": .string("delete_encounter"), "index": .number(Double(encounter.index))])) }
                    }.disabled(!model.canAuthor)
                }
            }
        }
    }
    @ViewBuilder private func inspectorField(_ object: SceneObject?, _ field: SceneField) -> some View {
        if let rgb = field.colour, rgb.count == 3 {
            VStack(alignment: .leading, spacing: 4) {
                ColorPicker(field.label, selection: Binding(get: { colourDrafts[field.key] ?? Color(.sRGBLinear, red: rgb[0], green: rgb[1], blue: rgb[2]) },
                    set: { colourDrafts[field.key] = $0 }), supportsOpacity: false).disabled(!model.canAuthor)
                if let draft = colourDrafts[field.key] {
                    HStack {
                        Button("Apply colour") { setColour(object, field, draft) }.disabled(!model.canAuthor)
                        Button("Discard") { colourDrafts.removeValue(forKey: field.key) }
                    }.controlSize(.small)
                }
            }
        } else if field.unit == "bool" {
            Toggle(field.label, isOn: Binding(get: { field.value != 0 }, set: { set(object, field, .unsigned($0 ? 1 : 0)) })).disabled(!model.canAuthor)
        } else if let options = field.options {
            Picker(field.label, selection: Binding(get: { Int(field.value) }, set: { set(object, field, .unsigned(UInt64($0))) })) {
                ForEach(Array(options.enumerated()), id: \.offset) { i, label in Text(label).tag(i) }
            }.disabled(!model.canAuthor)
        } else {
            VStack(alignment: .leading, spacing: 4) {
                Text(field.label + (field.unit.isEmpty ? "" : " · \(field.unit)")).font(.caption)
                HStack {
                    TextField(field.label, text: Binding(get: { fieldDrafts[field.key] ?? field.integerValue.map { String($0) } ?? number(field.value) }, set: { fieldDrafts[field.key] = $0 }))
                        .textFieldStyle(.roundedBorder).onSubmit {
                            guard let value = field.parsed(fieldDrafts[field.key] ?? "") else { fieldError = field.integer ? "Enter a nonnegative whole number." : "Enter a finite number."; return }
                            set(object, field, value)
                        }.onExitCommand { fieldDrafts.removeValue(forKey: field.key); fieldError = nil }
                    Stepper(field.label, onIncrement: { set(object, field, field.stepped(up: true)) }, onDecrement: { set(object, field, field.stepped(up: false)) }).labelsHidden().fixedSize()
                }.disabled(!model.canAuthor)
            }
        }
    }
    private func set(_ object: SceneObject?, _ field: SceneField, _ value: JSONValue?) {
        guard let value else { fieldError = "Value is outside the numeric range."; return }
        let submittedDraft = fieldDrafts[field.key]
        var edit: [String: JSONValue] = ["action": .string(object == nil ? "lighting" : "set"), "field": .string(field.key), "value": value]
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
            if await model.sendAuthoring(request), let document = model.editor {
                fieldDrafts.removeAll(); colourDrafts.removeAll(); fieldError = nil
                nameDraft = document.name; objectNameDraft = selected?.name ?? ""; fit(document)
            }
        }
    }
    private func save(asNew: Bool, play: Bool) {
        if !asNew, let path = model.editorPath { Task { await model.saveScene(path: path, play: play) }; return }
        let panel = NSSavePanel()
        panel.title = play ? "Save scene and play" : "Save scene"
        panel.nameFieldStringValue = "encounter.ron"
        panel.allowedContentTypes = [UTType(filenameExtension: "ron") ?? .data]
        panel.directoryURL = model.sceneDirectory
        panel.begin { result in
            if result == .OK, let url = panel.url { Task { await model.saveScene(path: url.path, play: play) } }
        }
    }
    private func fit(_ document: SceneDocument) {
        viewport.fit(document)
    }
}
