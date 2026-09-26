import SwiftUI
import AppKit
import UniformTypeIdentifiers

struct DebugView: View {
    @ObservedObject var model: Model

    var body: some View {
        Form {
            Section("Presentation") {
                Toggle("Collision outlines", isOn: Binding(
                    get: { model.state?.debug.collision ?? false },
                    set: { enabled in Task { await model.send(Request(op: .collision, enabled: enabled)) } }
                ))
                Toggle("Vsync", isOn: Binding(
                    get: { model.state?.debug.vsync ?? false },
                    set: { enabled in Task { await model.send(Request(op: .vsync, enabled: enabled)) } }
                )).disabled(model.state?.debug.vsync == nil)
                HStack {
                    Text("Camera height")
                    Spacer()
                    Text(model.state?.debug.view_height.map { "\(number($0)) m" } ?? "Unavailable")
                        .foregroundStyle(.secondary).monospacedDigit()
                    Button("Zoom in") { Task { await model.send(Request(op: .zoom, direction: .in)) } }
                    Button("Zoom out") { Task { await model.send(Request(op: .zoom, direction: .out)) } }
                }.disabled(model.state?.debug.view_height == nil)
            }.disabled(!model.editable)
            Section("Live readout") {
                if let state = model.state {
                    LabeledContent("Frame timing (smoothed)",
                        value: "\(state.debug.frame_ms.formatted(.number.precision(.fractionLength(2)))) ms · \(state.debug.fps.formatted(.number.precision(.fractionLength(0)))) fps")
                    LabeledContent("Scene", value: "\(state.debug.enemies) enemies · \(state.debug.instances) instances · Tick \(state.scenes.tick)")
                }
            }
            Section("Game screenshot") {
                Button("Save screenshot…", systemImage: "camera") { saveScreenshot() }
                    .disabled(!model.editable || model.state?.debug.capture.pending == true || model.state?.debug.vsync == nil)
                if let capture = model.state?.debug.capture {
                    if let error = capture.error {
                        Text(error).foregroundStyle(.red).textSelection(.enabled)
                    } else if let path = capture.path {
                        Text(capture.pending ? "Capturing… \(path)" : "Saved \(path)")
                            .foregroundStyle(.secondary).textSelection(.enabled)
                    }
                }
                Text("Captures the game frame, including gameplay HUD and any enabled outlines.")
                    .font(.caption).foregroundStyle(.secondary)
            }
        }.formStyle(.grouped)
    }

    private func saveScreenshot() {
        let panel = NSSavePanel()
        panel.allowedContentTypes = [.png]
        panel.nameFieldStringValue = "arpg.png"
        panel.begin { response in
            if response == .OK, let path = panel.url?.path {
                Task { await model.send(Request(op: .capture, path: path)) }
            }
        }
    }
}
