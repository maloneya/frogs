import SwiftUI
import AppKit

@main
struct CompanionApp: App {
    @NSApplicationDelegateAdaptor(AppDelegate.self) var delegate
    @StateObject private var model: Model

    init() {
        let args = CommandLine.arguments
        let path: String
        if let flag = args.firstIndex(of: "--socket"), args.indices.contains(flag + 1) {
            path = args[flag + 1]
        } else {
            path = ProcessInfo.processInfo.environment["ARPG_COMPANION_SOCKET"]
                ?? URL(fileURLWithPath: FileManager.default.currentDirectoryPath).appendingPathComponent("target/companion.sock").path
        }
        _model = StateObject(wrappedValue: Model(path: path))
    }

    var body: some Scene {
        Window("ARPG Companion", id: "companion") {
            CompanionView(model: model)
                .task {
                    let args = CommandLine.arguments
                    if let flag = args.firstIndex(of: "--scene"), args.indices.contains(flag + 1) {
                        await model.sendAuthoring(EditorRequest(action: "open", path: args[flag + 1]))
                    }
                    await model.poll()
                }
        }
        .defaultSize(width: 1120, height: 800)
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
        if let path = ProcessInfo.processInfo.environment["ARPG_COMPANION_CAPTURE"] {
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

enum ToolPage: String { case scenes, tuning, debug }

struct CompanionView: View {
    @ObservedObject var model: Model
    @State private var tuningGroup: String?
    @State private var page: ToolPage = {
        let args = CommandLine.arguments
        if let flag = args.firstIndex(of: "--tab"), args.indices.contains(flag + 1) {
            return ToolPage(rawValue: args[flag + 1]) ?? .scenes
        }
        return .scenes
    }()

    var body: some View {
        VStack(spacing: 0) {
            Picker("Tool", selection: $page) {
                Text("Scenes").tag(ToolPage.scenes)
                Text("Tuning").tag(ToolPage.tuning)
                Text("Debug").tag(ToolPage.debug)
            }.pickerStyle(.segmented).labelsHidden().frame(width: 360).padding(.top, 16)
            Group {
                switch page {
                case .scenes: ScenesView(model: model)
                case .tuning: TuningView(model: model, selection: $tuningGroup)
                case .debug: DebugView(model: model)
                }
            }.frame(maxWidth: .infinity, maxHeight: .infinity).padding(12)
            Divider()
            HStack(alignment: .top, spacing: 12) {
                Label(model.connected ? "Connected" : "Disconnected", systemImage: model.connected ? "circle.fill" : "circle")
                    .foregroundStyle(model.connected ? Color.green : Color.secondary)
                Text(page == .scenes && !model.connected ? "Game offline · scene editing is available" : model.notice)
                    .foregroundStyle(page != .scenes && model.failed ? Color.red : Color.secondary)
                    .lineLimit(3).textSelection(.enabled)
                Spacer(minLength: 0)
                if model.state?.busy == true { Text("Performance trial").foregroundStyle(.secondary) }
            }.font(.caption).padding(16)
        }.frame(minWidth: 960, minHeight: 640)
        .onChange(of: model.state?.groups, initial: true) { _, groups in
            if !(groups?.contains { $0 == tuningGroup } ?? false) { tuningGroup = groups?.first }
        }
    }
}
