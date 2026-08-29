import AppKit
import Combine
import Darwin
import Foundation

@MainActor
public final class DaemonSupervisor: ObservableObject {
    @Published public private(set) var observation: StateObservation

    private var reducer = SupervisorReducer()
    private var daemon: Process?
    private var controlWriter: FileHandle?
    private var eventReader: FileHandle?
    private var eventBuffer = Data()
    private var eventSequence: UInt64?
    private var exitPIDs = Set<Int32>()
    private var exitSource: DispatchSourceRead?
    private var kqueueFD: Int32 = -1
    private var drainTimeout: DispatchWorkItem?
    private var endpoint: String?
    private var bearerToken: String?
    private var restartWork: DispatchWorkItem?
    private var lifetimeAgentLabel: String?
    private var lifetimeAgentPlist: URL?
    private var lifetimePidFile: URL?
    private let workspaceRoot: String
    private let daemonPath: URL
    private let dashboardPath: URL?
    private let port: UInt16

    public init(workspaceRoot: String = FileManager.default.currentDirectoryPath,
                daemonPath: URL? = nil, dashboardPath: URL? = nil, port: UInt16 = 4317) {
        self.workspaceRoot = workspaceRoot
        self.daemonPath = daemonPath ?? Self.defaultDaemonPath()
        self.dashboardPath = dashboardPath
        self.port = port
        observation = reducer.observation
    }

    public func start() {
        guard daemon == nil else { return }
        reducer.start(); publish()
        launchProcess()
    }

    private func launchProcess() {
        let control = Pipe(), events = Pipe(), diagnostics = Pipe()
        let child = Process()
        child.executableURL = daemonPath
        child.standardInput = control
        child.standardOutput = events
        child.standardError = diagnostics
        child.environment = [:] // bearer token is sent only through inherited stdin.
        do { try child.run() }
        catch { reducer.failed(reason: "daemon_spawn_failed"); publish(); return }
        daemon = child
        do {
            if lifetimeAgentLabel == nil { try installLifetimeGuarantee() }
            try publishDaemonPid(child.processIdentifier)
        } catch {
            // Windows refuses to launch outside its kill-on-close job object; macOS
            // refuses to launch without the launchd kill guarantee for the same reason.
            reducer.failed(reason: "daemon_lifetime_guarantee_unavailable"); publish()
            terminateCurrent()
            return
        }
        controlWriter = control.fileHandleForWriting
        eventReader = events.fileHandleForReading
        eventReader?.readabilityHandler = { [weak self] handle in
            let data = handle.availableData
            Task { @MainActor [weak self] in self?.consume(data) }
        }
        diagnostics.fileHandleForReading.readabilityHandler = { handle in
            _ = handle.availableData // diagnostics are deliberately never mixed into stdout frames.
        }
        child.terminationHandler = { [weak self, weak child] process in
            Task { @MainActor [weak self, weak child] in
                guard let child else { return }
                self?.handleExit(child, status: process.terminationStatus, reason: "daemon_exited")
            }
        }
        installExitWatch(for: child)
        do {
            let token = Self.randomBearerToken()
            bearerToken = token
            try send(LaunchFrame(workspaceRoot: workspaceRoot, httpPort: port,
                                 bearerToken: token, parentPid: UInt32(getpid())))
        } catch {
            reducer.failed(reason: "daemon_protocol_invalid"); publish()
            terminateCurrent()
        }
    }

    public func restart() {
        restartWork?.cancel(); restartWork = nil
        guard observation.state != .running else { return }
        reducer.manualRestart(); publish()
        terminateCurrent()
        launchProcess()
    }

    public func drainAndQuit(completion: @escaping () -> Void) {
        guard let daemon else {
            removeLifetimeGuarantee()
            completion(); return
        }
        reducer.drain(); publish()
        try? send(CommandFrame(sequence: (eventSequence ?? 1) + 1))
        let finish = { [weak self] in
            self?.removeLifetimeGuarantee()
            completion()
        }
        var completed = false
        let timeout = DispatchWorkItem { [weak self, weak daemon] in
            Task { @MainActor in
                guard !completed else { return }
                completed = true
                if let self, let daemon, daemon.isRunning {
                    self.reducer.failed(reason: "daemon_drain_timeout"); self.publish(); self.terminateCurrent()
                }
                finish()
            }
        }
        drainTimeout = timeout
        DispatchQueue.main.asyncAfter(deadline: .now() + 7, execute: timeout)
        DispatchQueue.global(qos: .utility).async {
            daemon.waitUntilExit()
            Task { @MainActor in
                guard !completed else { return }
                completed = true; timeout.cancel(); finish()
            }
        }
    }

    public func launchDashboard() {
        guard let dashboardPath else { return }
        let bootstrap = Pipe()
        let process = Process(); process.executableURL = dashboardPath; process.standardInput = bootstrap
        do {
            try process.run()
            let payload: [String: String] = ["endpoint": endpoint ?? "", "token": bearerToken ?? ""]
            var data = try JSONSerialization.data(withJSONObject: payload); data.append(10)
            try bootstrap.fileHandleForWriting.write(contentsOf: data)
            try bootstrap.fileHandleForWriting.close()
        } catch { /* dashboard is on-demand; tray remains healthy */ }
    }

    private func consume(_ data: Data) {
        guard !data.isEmpty else { return }
        eventBuffer.append(data)
        while let newline = eventBuffer.firstIndex(of: 10) {
            let frame = eventBuffer.prefix(through: newline)
            eventBuffer.removeSubrange(...newline)
            do {
                let event = try FrameCodec.decodeEvent(Data(frame), previousSequence: eventSequence)
                eventSequence = event.sequence
                switch event.kind {
                case "ready":
                    endpoint = event.endpoint; reducer.ready(pid: Int32(event.pid), now: event.date); publish()
                case "draining": reducer.drain(now: event.date); publish()
                case "drained": reducer.failed(reason: "daemon_exited", now: event.date); publish()
                case "fatal":
                    reducer.failed(reason: event.reason ?? "daemon_ready_failed", now: event.date); publish()
                default: break
                }
            } catch { reducer.failed(reason: "daemon_protocol_invalid"); publish(); terminateCurrent() }
        }
    }

    private func send<T: Encodable>(_ frame: T) throws {
        try controlWriter?.write(contentsOf: FrameCodec.encode(frame))
    }

    private func handleExit(_ process: Process, status: Int32, reason: String) {
        guard daemon === process, !exitPIDs.contains(process.processIdentifier) else { return }
        exitPIDs.insert(process.processIdentifier)
        drainTimeout?.cancel(); drainTimeout = nil
        let signal = status < 0 ? -status : nil
        if observation.state == .draining { reducer.failed(reason: reason) }
        else {
            reducer.exited(code: status >= 0 ? status : nil, signal: signal)
            if reducer.observation.state == .backoff { scheduleAutomaticRestart() }
        }
        publish(); closeCurrentHandles()
        daemon = nil
        // The supervised daemon is gone: its published pid must not outlive it,
        // so the kill guarantee can never act on a recycled pid.
        if let lifetimePidFile { try? FileManager.default.removeItem(at: lifetimePidFile) }
    }

    private func scheduleAutomaticRestart() {
        restartWork?.cancel()
        let work = DispatchWorkItem { [weak self] in Task { @MainActor in self?.start() } }
        restartWork = work
        DispatchQueue.main.asyncAfter(deadline: .now() + 1, execute: work)
    }

    private func terminateCurrent() {
        if let daemon, daemon.isRunning { daemon.terminate() }
        closeCurrentHandles()
        daemon = nil
    }

    private func closeCurrentHandles() {
        eventReader?.readabilityHandler = nil; try? eventReader?.close(); eventReader = nil
        try? controlWriter?.close(); controlWriter = nil
        exitSource?.cancel(); exitSource = nil
        if kqueueFD >= 0 { close(kqueueFD); kqueueFD = -1 }
    }

    // launchd kill guarantee — the macOS counterpart of the Windows
    // JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE coupling. A per-tray launchd agent
    // (KeepAlive, gui domain of this tray session) waits for this tray process
    // to die, then terminates the last published daemon pid and removes itself.
    // Unlike the Windows job object this is poll-based (0.2s) and installed just
    // after spawn: a tray crash inside that install window is covered only if
    // the daemon honours the LaunchFrame parentPid it receives.
    private func installLifetimeGuarantee() throws {
        let trayPid = getpid()
        let uid = getuid()
        let label = "app.membrane.tray.kill-guarantee.\(trayPid)"
        let pidFile = FileManager.default.temporaryDirectory
            .appendingPathComponent("membrane-tray-daemon-\(trayPid).pid")
        let script = Self.lifetimeGuaranteeScript(trayPid: trayPid, pidFile: pidFile.path,
                                                  label: label, uid: uid)
        let plist: [String: Any] = [
            "Label": label,
            "ProgramArguments": ["/bin/sh", "-c", script],
            "KeepAlive": true,
            "RunAtLoad": true,
            "ProcessType": "Background",
        ]
        let data = try PropertyListSerialization.data(fromPropertyList: plist, format: .xml, options: 0)
        let agentsDirectory = FileManager.default.homeDirectoryForCurrentUser
            .appendingPathComponent("Library/LaunchAgents", isDirectory: true)
        let plistURL = agentsDirectory.appendingPathComponent("\(label).plist")
        try FileManager.default.createDirectory(at: agentsDirectory, withIntermediateDirectories: true)
        try data.write(to: plistURL)
        try? Self.launchctl(["bootout", "gui/\(uid)/\(label)"]) // clear a stale instance before re-bootstrap
        try Self.launchctl(["bootstrap", "gui/\(uid)", plistURL.path])
        lifetimeAgentLabel = label
        lifetimeAgentPlist = plistURL
        lifetimePidFile = pidFile
    }

    private func publishDaemonPid(_ pid: Int32) throws {
        guard let pidFile = lifetimePidFile else {
            throw NSError(domain: "DaemonSupervisor", code: 1,
                          userInfo: [NSLocalizedDescriptionKey: "lifetime guarantee not installed"])
        }
        try Data("\(pid)\n".utf8).write(to: pidFile)
    }

    /// Best-effort removal once the guarantee is discharged (drained daemon, tray
    /// exiting): a leftover agent self-cleans anyway — it bootouts itself once the
    /// pid file it reads is gone or names a dead process.
    private func removeLifetimeGuarantee() {
        guard let label = lifetimeAgentLabel else { return }
        try? Self.launchctl(["bootout", "gui/\(getuid())/\(label)"])
        if let plist = lifetimeAgentPlist { try? FileManager.default.removeItem(at: plist) }
        if let pidFile = lifetimePidFile { try? FileManager.default.removeItem(at: pidFile) }
        lifetimeAgentLabel = nil
        lifetimeAgentPlist = nil
        lifetimePidFile = nil
    }

    private static func lifetimeGuaranteeScript(trayPid: Int32, pidFile: String, label: String, uid: UInt32) -> String {
        """
        while kill -0 \(trayPid) 2>/dev/null; do sleep 0.2; done
        daemon=$(cat '\(pidFile)' 2>/dev/null)
        if [ -n "$daemon" ]; then
          kill "$daemon" 2>/dev/null
          sleep 2
          kill -9 "$daemon" 2>/dev/null
        fi
        /bin/launchctl bootout gui/\(uid)/\(label) 2>/dev/null
        """
    }

    private static func launchctl(_ arguments: [String]) throws {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/bin/launchctl")
        process.arguments = arguments
        process.standardOutput = FileHandle.nullDevice
        process.standardError = FileHandle.nullDevice
        try process.run()
        process.waitUntilExit()
        guard process.terminationStatus == 0 else {
            throw NSError(domain: "DaemonSupervisor.launchctl", code: Int(process.terminationStatus),
                          userInfo: [NSLocalizedDescriptionKey: "launchctl \(arguments.first ?? "") failed"])
        }
    }

    private func publish() { observation = reducer.observation }

    private func installExitWatch(for process: Process) {
        let descriptor = kqueue(); guard descriptor >= 0 else { return }
        kqueueFD = descriptor
        var registration = kevent(ident: UInt(process.processIdentifier), filter: Int16(EVFILT_PROC),
                                  flags: UInt16(EV_ADD | EV_ENABLE), fflags: UInt32(NOTE_EXIT), data: 0, udata: nil)
        guard kevent(descriptor, &registration, 1, nil, 0, nil) == 0 else { close(descriptor); kqueueFD = -1; return }
        let source = DispatchSource.makeReadSource(fileDescriptor: descriptor, queue: DispatchQueue.global(qos: .userInitiated))
        source.setEventHandler { [weak self, weak process] in
            var event = kevent()
            guard kevent(descriptor, nil, 0, &event, 1, nil) > 0, let process else { return }
            Task { @MainActor [weak self] in self?.handleExit(process, status: process.terminationStatus, reason: "daemon_exited") }
        }
        source.setCancelHandler { close(descriptor) }
        exitSource = source; source.resume()
    }

    private static func randomBearerToken() -> String {
        (0..<32).map { _ in String(format: "%02x", UInt8.random(in: 0...255)) }.joined()
    }

    private static func defaultDaemonPath() -> URL {
        if let raw = ProcessInfo.processInfo.environment["MEMBRANE_DAEMON_PATH"] { return URL(fileURLWithPath: raw) }
        return Bundle.main.bundleURL.appendingPathComponent("Contents/MacOS/membrane-daemon")
    }
}
