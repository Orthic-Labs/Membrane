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
        guard let daemon else { completion(); return }
        reducer.drain(); publish()
        try? send(CommandFrame(sequence: (eventSequence ?? 1) + 1))
        var completed = false
        let timeout = DispatchWorkItem { [weak self, weak daemon] in
            Task { @MainActor in
                guard !completed else { return }
                completed = true
                if let self, let daemon, daemon.isRunning {
                    self.reducer.failed(reason: "daemon_drain_timeout"); self.publish(); self.terminateCurrent()
                }
                completion()
            }
        }
        drainTimeout = timeout
        DispatchQueue.main.asyncAfter(deadline: .now() + 7, execute: timeout)
        DispatchQueue.global(qos: .utility).async {
            daemon.waitUntilExit()
            Task { @MainActor in
                guard !completed else { return }
                completed = true; timeout.cancel(); completion()
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
