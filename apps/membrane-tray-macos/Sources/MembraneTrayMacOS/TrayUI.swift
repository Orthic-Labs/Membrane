import AppKit
import Combine
import ServiceManagement
import SwiftUI

@main
struct MembraneTrayMacOSApp: App {
    @NSApplicationDelegateAdaptor(TrayApplicationDelegate.self) private var delegate
    var body: some Scene { Settings { EmptyView() } }
}

@MainActor
final class TrayApplicationDelegate: NSObject, NSApplicationDelegate {
    let supervisor = DaemonSupervisor(
        workspaceRoot: ProcessInfo.processInfo.environment["MEMBRANE_WORKSPACE_ROOT"] ?? FileManager.default.currentDirectoryPath,
        daemonPath: ProcessInfo.processInfo.environment["MEMBRANE_DAEMON_PATH"].map(URL.init(fileURLWithPath:)),
        dashboardPath: ProcessInfo.processInfo.environment["MEMBRANE_DASHBOARD_PATH"].map(URL.init(fileURLWithPath:)))
    private var statusItem: NSStatusItem!
    private var popover: NSPopover!
    private var firstRunWindow: NSWindow?

    func applicationDidFinishLaunching(_ notification: Notification) {
        NSApp.setActivationPolicy(.accessory)
        statusItem = NSStatusBar.system.statusItem(withLength: NSStatusItem.squareLength)
        statusItem.button?.target = self; statusItem.button?.action = #selector(togglePopover(_:))
        statusItem.button?.toolTip = "Membrane — Starting"
        popover = NSPopover(); popover.behavior = .transient; popover.animates = true
        popover.contentSize = NSSize(width: 340, height: 310)
        popover.contentViewController = NSHostingController(rootView: TrayPopover(supervisor: supervisor, launchDashboard: { [weak self] in self?.supervisor.launchDashboard() }, quit: { NSApp.terminate(nil) }))
        updateIcon(supervisor.observation)
        supervisor.$observation.sink { [weak self] observation in self?.updateIcon(observation) }.store(in: &subscriptions)
        supervisor.start()
        if !UserDefaults.standard.bool(forKey: "membrane.firstRunCompleted") { showFirstRun() }
    }

    private var subscriptions = Set<AnyCancellable>()

    @objc private func togglePopover(_ sender: Any?) {
        guard let button = statusItem.button else { return }
        if popover.isShown { popover.performClose(sender); return }
        // NSStatusItem's native anchoring tracks menu bars on every screen; edge selection
        // follows AppKit's work-area-aware placement instead of cursor position.
        popover.show(relativeTo: button.bounds, of: button, preferredEdge: .minY)
    }

    private func updateIcon(_ observation: StateObservation) {
        guard let button = statusItem?.button else { return }
        let symbol: String
        let color: NSColor
        switch observation.state {
        case .running: symbol = "square.fill"; color = .systemGreen
        case .starting, .draining: symbol = "square.lefthalf.filled"; color = .systemOrange
        default: symbol = "square"; color = .systemRed
        }
        let image = NSImage(systemSymbolName: symbol, accessibilityDescription: "Membrane (observation.state.title)")
        image?.isTemplate = false
        button.image = image?.withSymbolConfiguration(.init(paletteColors: [color]))
        button.toolTip = "Membrane — \(observation.state.title) · \(observation.reason)"
    }

    private func showFirstRun() {
        let view = FirstRunView(enableLogin: { enabled in
            StartupManager.setEnabled(enabled)
            UserDefaults.standard.set(true, forKey: "membrane.firstRunCompleted")
        }, openDashboard: { [weak self] in self?.supervisor.launchDashboard(); self?.firstRunWindow?.close() })
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 440, height: 330), styleMask: [.titled, .closable], backing: .buffered, defer: false)
        window.title = "Welcome to Membrane"; window.contentView = NSHostingView(rootView: view); window.center(); window.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true); firstRunWindow = window
    }
}

struct TrayPopover: View {
    @ObservedObject var supervisor: DaemonSupervisor
    let launchDashboard: () -> Void
    let quit: () -> Void

    var body: some View {
        let observation = supervisor.observation
        VStack(alignment: .leading, spacing: 0) {
            HStack {
                Image(systemName: "square.fill").foregroundStyle(statusColor(observation.state))
                Text("Membrane").font(.system(size: 17, weight: .semibold)); Spacer()
                Text(observation.state.title.uppercased()).font(.system(size: 10, weight: .bold, design: .monospaced)).foregroundStyle(statusColor(observation.state))
            }.padding(.bottom, 18)
            Text(observation.reason).font(.system(size: 12, design: .monospaced)).foregroundStyle(.secondary)
            Divider().padding(.vertical, 14)
            infoRow("Generation", "\(observation.generation)")
            infoRow("PID", observation.pid.map(String.init) ?? "—")
            infoRow("Observed", observation.observedAt.formatted(date: .omitted, time: .standard))
            if let code = observation.exitCode { infoRow("Exit", "\(code)") }
            Spacer(minLength: 18)
            if observation.state == .running {
                Button(action: launchDashboard) { Label("Open dashboard", systemImage: "rectangle.on.rectangle") }.buttonStyle(.borderedProminent).controlSize(.large)
            } else if observation.state != .starting && observation.state != .draining {
                Button(action: supervisor.restart) { Label("Restart", systemImage: "arrow.clockwise") }.buttonStyle(.borderedProminent).controlSize(.large)
            }
            HStack { Spacer(); Button("Quit", action: quit).buttonStyle(.plain).foregroundStyle(.secondary) }
                .padding(.top, 12)
        }
        .padding(20).frame(width: 340, height: 310).background(Color(nsColor: .windowBackgroundColor))
    }

    private func infoRow(_ label: String, _ value: String) -> some View {
        HStack { Text(label).foregroundStyle(.secondary); Spacer(); Text(value).font(.system(size: 12, design: .monospaced)) }.padding(.vertical, 3)
    }
    private func statusColor(_ state: TrayState) -> Color {
        switch state { case .running: .green; case .starting, .draining: .orange; default: .red }
    }
}

struct FirstRunView: View {
    let enableLogin: (Bool) -> Void
    let openDashboard: () -> Void
    @State private var login = true
    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text("Membrane is ready").font(.title2.bold())
            Text("Membrane runs as a headless child of this visible menu-bar app. Closing this tray also stops its runtime.").foregroundStyle(.secondary)
            Toggle("Launch Membrane at login", isOn: $login).onChange(of: login) { value in enableLogin(value) }
            Spacer()
            HStack { Button("Open dashboard", action: openDashboard).buttonStyle(.borderedProminent); Spacer(); Button("Done") { enableLogin(login); UserDefaults.standard.set(true, forKey: "membrane.firstRunCompleted") } }
        }.padding(26).frame(width: 440, height: 330)
    }
}

enum StartupManager {
    static func setEnabled(_ enabled: Bool) {
        if #available(macOS 13.0, *) {
            do { if enabled { try SMAppService.mainApp.register() } else { try SMAppService.mainApp.unregister() } } catch { /* status remains visible; user can retry in first-run */ }
        }
    }
}
