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
            if let perf = model.state?.debug.performance, let frame = perf.frame_id {
                Section("CPU frame \(frame)") {
                    timing("Game steps", perf.game_step_ms)
                    timing("Presentation preparation", perf.presentation_ms)
                    timing("Surface acquisition", perf.acquire_ms)
                    timing("Uploads and packing", perf.upload_ms)
                    timing("Command encoding", perf.encode_ms)
                    timing("Submission", perf.submit_ms)
                    timing("Present", perf.present_ms)
                }
                Section("GPU passes") {
                    if let frame = perf.gpu_frame_id {
                        Text("Completed GPU frame \(frame)").foregroundStyle(.secondary)
                        timing("Directional shadow", perf.gpu_directional_shadow_ms)
                        timing("Fire shadow", perf.gpu_fire_shadow_ms)
                        timing("Main pass", perf.gpu_main_ms)
                        timing("Pass span including gaps", perf.gpu_span_ms)
                    } else {
                        Text("No completed GPU timing available for this window.").foregroundStyle(.secondary)
                    }
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

    @ViewBuilder
    private func timing(_ label: String, _ value: Double?) -> some View {
        if let value {
            LabeledContent(label, value: "\(value.formatted(.number.precision(.fractionLength(3)))) ms")
                .monospacedDigit()
        }
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
