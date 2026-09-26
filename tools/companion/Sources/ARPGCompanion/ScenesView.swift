import SwiftUI

struct ScenesView: View {
    @ObservedObject var model: Model
    @Binding var selection: String?

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            HStack {
                VStack(alignment: .leading, spacing: 4) {
                    Text(model.state?.scenes.current ?? "No scene selected").font(.title2.bold())
                    Text("Current playtest • Run \(model.state?.scenes.run ?? 0)").foregroundStyle(.secondary)
                }
                Spacer()
                Button("Restart current") { Task { await model.action(.restart) } }
                Button("Default horde") { Task { await model.action(.defaultScene) } }
            }.disabled(!model.editable)
            Divider()
            HStack {
                Text("Scene files").font(.headline)
                Spacer()
                Button("Refresh files", systemImage: "arrow.clockwise") { Task { await model.action(.refreshScenes) } }
                    .disabled(!model.editable)
            }
            if let error = model.state?.scenes.error {
                Text(error).foregroundStyle(.red).textSelection(.enabled)
            }
            List(model.state?.scenes.files ?? [], selection: $selection) { file in
                VStack(alignment: .leading, spacing: 3) {
                    Text(file.label)
                    Text(file.path).font(.caption).foregroundStyle(.secondary)
                }.padding(.vertical, 3).tag(file.path)
            }.listStyle(.bordered).disabled(!model.editable)
            HStack {
                Text("Start reads the file again. Restart repeats the cached scene.\nThe game keeps running while you use the companion.")
                    .font(.caption).foregroundStyle(.secondary)
                Spacer()
                Button("Start selected") {
                    if let selection { Task { await model.send(Request(op: .startScene, path: selection)) } }
                }.buttonStyle(.borderedProminent)
                    .disabled(!model.editable || !(model.state?.scenes.files.contains { $0.path == selection } ?? false))
            }
        }.padding(20)
    }
}
