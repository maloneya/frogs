import SwiftUI

struct TuningView: View {
    @ObservedObject var model: Model
    @Binding var selection: String?
    private var group: String? { selection ?? model.state?.groups.first }

    var body: some View {
        VStack(spacing: 0) {
            HSplitView {
                VStack(alignment: .leading) {
                    Text("Types").font(.headline).padding([.top, .leading], 16)
                    List(model.state?.groups ?? [], id: \.self, selection: $selection) { group in
                        Text(group).tag(group)
                    }
                    .listStyle(.plain)
                    .scrollContentBackground(.hidden)
                }.frame(minWidth: 150, idealWidth: 170, maxWidth: 240)
                ScrollView {
                    VStack(alignment: .leading, spacing: 20) {
                        HStack {
                            Text(group ?? "Runtime tuning").font(.title2.bold())
                            Spacer()
                            Text(model.state?.unsaved == true ? "Unsaved changes" : "Saved values")
                                .font(.caption).foregroundStyle(.secondary)
                        }
                        if let state = model.state {
                            ForEach(state.fields.filter { $0.group == group }) { field in
                                VStack(alignment: .leading, spacing: 8) {
                                    attributeRow(field)
                                    if let note = field.note {
                                        Text(note).font(.callout).foregroundStyle(.secondary)
                                    }
                                }
                            }
                        } else {
                            ContentUnavailableView("Waiting for the game", systemImage: "gamecontroller", description: Text("Start the game from this checkout. Settings appear when it connects."))
                        }
                    }.padding(24)
                }
            }
            Divider()
            VStack(alignment: .leading, spacing: 10) {
                HStack {
                    Button("Save tuning") { Task { await model.action(.save) } }.keyboardShortcut("s")
                    Button("Revert saved") { Task { await model.action(.revert) } }
                    Button("Reset defaults") { Task { await model.action(.defaults) } }
                    Spacer()
                    Button("Restart scene") { Task { await model.action(.restart) } }.disabled(model.state?.scenes.current == nil)
                }.disabled(!model.editable)
                Text(model.state?.path ?? model.socketPath).font(.caption2).foregroundStyle(.secondary)
                    .textSelection(.enabled).lineLimit(1).truncationMode(.middle)
            }.padding(16)
        }
    }

    private func attributeRow(_ field: Attribute) -> some View {
        HStack(spacing: 12) {
            VStack(alignment: .leading, spacing: 4) {
                Text(field.label + (field.value != field.saved ? " *" : ""))
                Text("\(number(field.min))–\(number(field.max)) \(field.unit)")
                    .font(.caption).foregroundStyle(.secondary)
            }
            Spacer()
            TextField(field.label, text: Binding(
                get: { model.drafts[field.key] ?? number(field.value) },
                set: { model.drafts[field.key] = $0 }
            ))
            .textFieldStyle(.roundedBorder).multilineTextAlignment(.trailing)
            .frame(width: 100).accessibilityLabel(field.label)
            .onSubmit { Task { await model.submit(field) } }
            .onExitCommand { model.drafts.removeValue(forKey: field.key) }
            Stepper(field.label, onIncrement: {
                Task { await model.set(field, value: min(field.max, field.value + field.step)) }
            }, onDecrement: {
                Task { await model.set(field, value: max(field.min, field.value - field.step)) }
            }).labelsHidden().fixedSize()
            Button("Reset") { Task { await model.set(field, value: field.default) } }
                .help("Restore built-in value: \(number(field.default)) \(field.unit)")
        }.disabled(!model.editable)
    }
}

func number(_ value: Double) -> String {
    value.formatted(.number.locale(Locale(identifier: "en_US_POSIX")).grouping(.never).precision(.significantDigits(1...6)))
}
