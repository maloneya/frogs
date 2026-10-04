import SwiftUI

struct EncounterEditor: View {
    @ObservedObject var model: Model
    let document: SceneDocument
    @State private var form: EncounterForm
    @Environment(\.dismiss) private var dismiss

    init(model: Model, document: SceneDocument, initial: EncounterForm) {
        self.model = model; self.document = document; _form = State(initialValue: initial)
    }
    private var triggers: [SceneObject] { document.objects.filter { $0.kind == "body" && $0.value("interactable") != 0 } }
    private var sources: [SceneObject] { document.objects.filter { $0.kind == "source" && $0.value("fixed") == 0 } }

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text(form.index == nil ? "New encounter" : "Edit encounter").font(.title2.bold())
            ScrollView {
                VStack(alignment: .leading, spacing: 14) {
                    TextField("Encounter name", text: $form.name).textFieldStyle(.roundedBorder)
                    Picker("When activated", selection: $form.trigger) {
                        Text("Choose a trigger").tag(Int?.none)
                        ForEach(triggers) { object in Text(object.label).tag(Optional(object.index)) }
                    }
                    if triggers.isEmpty { Text("Place a switch, or enable Can interact on a body, before creating the encounter.").foregroundStyle(.secondary) }
                    Text("Enemy sources").font(.headline)
                    if sources.isEmpty { Text("Place a source that emits movable enemies first.").foregroundStyle(.secondary) }
                    ForEach(sources) { source in
                        let owner = document.encounters.first { $0.index != form.index && $0.sources.contains(source.index) }
                        Toggle(source.label + (owner.map { " · used by \($0.name)" } ?? ""), isOn: Binding(
                            get: { form.sources.contains(source.index) },
                            set: { enabled in if enabled { form.sources.append(source.index) } else { form.sources.removeAll { $0 == source.index } } }))
                            .disabled(owner != nil)
                    }
                    Text("Selected sources start disabled and activate together. Removing a source from this encounter leaves its initial enablement unchanged.")
                        .font(.caption).foregroundStyle(.secondary)
                    HStack {
                        Text("Defeat target"); TextField("Enemies", text: $form.count).frame(width: 100)
                        Text("Deadline"); TextField("Ticks", text: $form.ticks).frame(width: 110); Text("ticks")
                    }.textFieldStyle(.roundedBorder)
                    rewardChoices("On start", actions: $form.onStart)
                    rewardChoices("On success", actions: $form.onSuccess)
                    rewardChoices("On timeout", actions: $form.onFailure)
                    Text("Terminal outcomes stop these sources; spawned enemies remain. Door rewards release the physical latch; chest rewards enable interaction.")
                        .font(.caption).foregroundStyle(.secondary)
                }.disabled(!model.canAuthor)
            }
            if model.editorFailed { Text(model.editorNotice).foregroundStyle(.red).textSelection(.enabled) }
            HStack {
                Button("Cancel") { dismiss() }.keyboardShortcut(.cancelAction).disabled(model.busy)
                Spacer()
                Button("Apply encounter") {
                    guard let edit = form.edit else { return }
                    Task {
                        if await model.editScene(edit) {
                            model.editorEncounter = form.index ?? model.editor?.encounters.last?.index
                            dismiss()
                        }
                    }
                }.keyboardShortcut(.defaultAction).disabled(!model.canAuthor || form.edit == nil)
            }
        }.padding(24).frame(width: 620, height: 650)
    }

    private func rewardChoices(_ title: String, actions: Binding<[JSONValue]>) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(title).font(.headline)
            if document.rewards.isEmpty { Text("Place a door or chest to add an unlock action.").font(.caption).foregroundStyle(.secondary) }
            ForEach(document.rewards) { reward in
                let owner = document.encounters.first { encounter in
                    encounter.index != form.index && (encounter.on_start + encounter.on_success + encounter.on_failure).contains(reward.action)
                }
                Toggle("Unlock \(reward.label)" + (owner.map { " · used by \($0.name)" } ?? ""), isOn: Binding(
                    get: { actions.wrappedValue.contains(reward.action) },
                    set: { enabled in if enabled { actions.wrappedValue.append(reward.action) } else { actions.wrappedValue.removeAll { $0 == reward.action } } }))
                    .disabled(owner != nil)
            }
        }
    }
}
