import SwiftUI
import AppKit

@main
struct TuningApp: App {
    @NSApplicationDelegateAdaptor(AppDelegate.self) var delegate
    @StateObject private var model: Model

    init() {
        let args = CommandLine.arguments
        let path: String
        if let flag = args.firstIndex(of: "--socket"), args.indices.contains(flag + 1) {
            path = args[flag + 1]
        } else {
            path = ProcessInfo.processInfo.environment["ARPG_TUNING_SOCKET"]
                ?? URL(fileURLWithPath: FileManager.default.currentDirectoryPath).appendingPathComponent("target/tuning.sock").path
        }
        _model = StateObject(wrappedValue: Model(path: path))
    }

    var body: some Scene {
        Window("ARPG Tuning", id: "tuning") {
            TuningView(model: model)
                .task { await model.poll() }
        }
        .defaultSize(width: 800, height: 560)
        .windowResizability(.contentMinSize)
    }
}

@MainActor
final class AppDelegate: NSObject, NSApplicationDelegate {
    func applicationDidFinishLaunching(_ notification: Notification) {
        NSApp.setActivationPolicy(.regular)
        NSApp.activate(ignoringOtherApps: true)
        // Capture our own rendered view for visual checks; no desktop permission
        // or synthetic input. This is opt-in and never changes settings.
        if let path = ProcessInfo.processInfo.environment["ARPG_TUNING_CAPTURE"] {
            Task {
                try? await Task.sleep(for: .seconds(3))
                guard let view = NSApp.windows.first(where: { $0.isVisible })?.contentView,
                      let bitmap = view.bitmapImageRepForCachingDisplay(in: view.bounds) else { return }
                view.effectiveAppearance.performAsCurrentDrawingAppearance {
                    view.cacheDisplay(in: view.bounds, to: bitmap)
                    // Composite transparent view regions over the native window backing.
                    guard let cg = bitmap.cgImage,
                          let context = CGContext(data: nil, width: cg.width, height: cg.height,
                              bitsPerComponent: 8, bytesPerRow: 0, space: CGColorSpaceCreateDeviceRGB(),
                              bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue) else { return }
                    context.setFillColor(NSColor.windowBackgroundColor.cgColor)
                    let rect = CGRect(x: 0, y: 0, width: cg.width, height: cg.height)
                    context.fill(rect)
                    context.draw(cg, in: rect)
                    guard let image = context.makeImage() else { return }
                    try? NSBitmapImageRep(cgImage: image).representation(using: .png, properties: [:])?
                        .write(to: URL(fileURLWithPath: path))
                }
            }
        }
    }
    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { true }
}

struct TuningView: View {
    @ObservedObject var model: Model
    @State private var selection: String?
    private var group: String? { selection ?? model.state?.groups.first }
    private var editable: Bool { model.connected && !model.busy && model.state?.busy == false }

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
                    Label(model.connected ? "Connected" : "Disconnected", systemImage: model.connected ? "circle.fill" : "circle")
                        .foregroundStyle(model.connected ? Color.green : Color.secondary)
                    Text(model.notice).foregroundStyle(model.failed ? Color.red : Color.secondary)
                        .lineLimit(3).textSelection(.enabled)
                    Spacer(minLength: 0)
                    if model.state?.busy == true { Text("Performance trial").foregroundStyle(.secondary) }
                }.font(.caption)
                HStack {
                    Button("Save tuning") { Task { await model.action(.save) } }.keyboardShortcut("s")
                    Button("Revert saved") { Task { await model.action(.revert) } }
                    Button("Reset defaults") { Task { await model.action(.defaults) } }
                    Spacer()
                    Button("Restart encounter") { Task { await model.action(.restart) } }
                }.disabled(!editable)
                Text(model.state?.path ?? model.socketPath).font(.caption2).foregroundStyle(.secondary)
                    .textSelection(.enabled).lineLimit(1).truncationMode(.middle)
            }.padding(16)
        }.frame(minWidth: 740, minHeight: 500)
        .onChange(of: model.state?.groups, initial: true) { _, groups in
            if !((groups ?? []).contains { $0 == selection }) { selection = groups?.first }
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
        }.disabled(!editable)
    }
}

func number(_ value: Double) -> String {
    value.formatted(.number.locale(Locale(identifier: "en_US_POSIX")).grouping(.never).precision(.significantDigits(1...6)))
}
