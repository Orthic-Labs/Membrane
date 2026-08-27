import Foundation

public enum TrayState: String, Equatable, Codable, Sendable {
    case starting, running, draining, stopped, backoff, crashLoop

    public var title: String {
        switch self {
        case .starting: "Starting"
        case .running: "Running"
        case .draining: "Stopping"
        case .stopped: "Offline"
        case .backoff: "Restarting"
        case .crashLoop: "Crash loop"
        }
    }

    public var reason: String {
        switch self {
        case .starting: "daemon_starting"
        case .running: "live"
        case .draining: "daemon_draining"
        case .stopped: "daemon_exited"
        case .backoff: "daemon_restart_backoff"
        case .crashLoop: "daemon_crash_loop"
        }
    }

    public var isHealthy: Bool { self == .running }
}

public struct StateObservation: Equatable, Sendable {
    public let state: TrayState
    public let reason: String
    public let generation: Int
    public let pid: Int32?
    public let observedAt: Date
    public let exitCode: Int32?
    public let signal: Int32?

    public init(state: TrayState, reason: String? = nil, generation: Int,
                pid: Int32? = nil, observedAt: Date = Date(),
                exitCode: Int32? = nil, signal: Int32? = nil) {
        self.state = state
        self.reason = reason ?? state.reason
        self.generation = generation
        self.pid = pid
        self.observedAt = observedAt
        self.exitCode = exitCode
        self.signal = signal
    }
}

public struct CrashLoopTracker: Sendable {
    public static let threshold = 3
    public static let window: TimeInterval = 60
    private(set) public var failures: [Date] = []
    private(set) public var runStartedAt: Date?

    public init() {}

    public mutating func started(at date: Date) {
        if let previousStart = runStartedAt, date.timeIntervalSince(previousStart) >= Self.window { failures.removeAll() }
        runStartedAt = date
    }

    public mutating func unexpectedExit(at date: Date) -> Bool {
        if let runStartedAt, date.timeIntervalSince(runStartedAt) >= Self.window { failures.removeAll() }
        failures = failures.filter { date.timeIntervalSince($0) < Self.window }
        failures.append(date)
        runStartedAt = nil
        return failures.count >= Self.threshold
    }

    public mutating func manualRestart() { failures.removeAll(); runStartedAt = nil }
}

public struct SupervisorReducer: Sendable {
    public private(set) var observation: StateObservation
    public private(set) var crashLoop = CrashLoopTracker()

    public init(now: Date = Date()) {
        observation = StateObservation(state: .stopped, generation: 0, observedAt: now)
    }

    public mutating func start(now: Date = Date()) {
        crashLoop.started(at: now)
        observation = StateObservation(state: .starting, generation: observation.generation + 1, observedAt: now)
    }

    public mutating func ready(pid: Int32, now: Date = Date()) {
        crashLoop.started(at: now)
        observation = StateObservation(state: .running, generation: observation.generation,
                                       pid: pid, observedAt: now)
    }

    public mutating func drain(now: Date = Date()) {
        observation = StateObservation(state: .draining, generation: observation.generation,
                                       pid: observation.pid, observedAt: now)
    }

    public mutating func failed(reason: String, now: Date = Date()) {
        observation = StateObservation(state: .stopped, reason: reason, generation: observation.generation,
                                       observedAt: now, exitCode: nil, signal: nil)
    }

    @discardableResult
    public mutating func exited(code: Int32?, signal: Int32?, now: Date = Date()) -> TrayState {
        let loop = crashLoop.unexpectedExit(at: now)
        let state: TrayState = loop ? .crashLoop : .backoff
        let reason = loop ? "daemon_crash_loop" : "daemon_exited"
        observation = StateObservation(state: state, reason: reason, generation: observation.generation,
                                       observedAt: now, exitCode: code, signal: signal)
        return state
    }

    public mutating func manualRestart(now: Date = Date()) {
        crashLoop.manualRestart()
        start(now: now)
    }
}

public enum DaemonFrameError: Error, Equatable {
    case oversize
    case invalidUTF8
    case invalidLine
    case invalidJSON
    case unknownKind
    case sequenceRegression
}

public struct LaunchFrame: Codable, Equatable, Sendable {
    public let schemaVersion: Int
    public let sequence: UInt64
    public let kind: String
    public let workspaceRoot: String
    public let httpPort: UInt16
    public let bearerToken: String
    public let parentPid: UInt32

    public init(workspaceRoot: String, httpPort: UInt16, bearerToken: String, parentPid: UInt32) {
        schemaVersion = 1; sequence = 1; kind = "launch"
        self.workspaceRoot = workspaceRoot; self.httpPort = httpPort
        self.bearerToken = bearerToken; self.parentPid = parentPid
    }
}

public struct CommandFrame: Codable, Equatable, Sendable {
    public let schemaVersion: Int = 1
    public let sequence: UInt64
    public let kind: String = "drain"
}

public struct EventFrame: Codable, Equatable, Sendable {
    public let schemaVersion: Int
    public let sequence: UInt64
    public let kind: String
    public let pid: UInt32
    public let observedAtUnixMs: UInt64
    public let endpoint: String?
    public let reason: String?
    public var date: Date { Date(timeIntervalSince1970: Double(observedAtUnixMs) / 1_000) }
}

public enum FrameCodec {
    public static let maxBytes = 16 * 1024

    public static func encode<T: Encodable>(_ value: T) throws -> Data {
        let encoder = JSONEncoder()
        encoder.outputFormatting = []
        var data = try encoder.encode(value); data.append(10)
        guard data.count <= maxBytes else { throw DaemonFrameError.oversize }
        return data
    }

    public static func decodeEvent(_ data: Data, previousSequence: UInt64?) throws -> EventFrame {
        guard data.count <= maxBytes else { throw DaemonFrameError.oversize }
        guard let text = String(data: data, encoding: .utf8) else { throw DaemonFrameError.invalidUTF8 }
        guard text.last == "\n", text.dropLast().first != "\n", !text.dropLast().contains("\r") else {
            throw DaemonFrameError.invalidLine
        }
        guard let object = try? JSONSerialization.jsonObject(with: Data(text.dropLast().utf8)),
              let dictionary = object as? [String: Any] else { throw DaemonFrameError.invalidJSON }
        let allowed = Set(["schemaVersion", "sequence", "kind", "pid", "observedAtUnixMs", "endpoint", "reason"])
        guard Set(dictionary.keys).isSubset(of: allowed) else { throw DaemonFrameError.invalidJSON }
        let event: EventFrame
        do { event = try JSONDecoder().decode(EventFrame.self, from: Data(text.dropLast().utf8)) }
        catch { throw DaemonFrameError.invalidJSON }
        guard event.schemaVersion == 1, event.sequence > 0, event.pid > 0 else { throw DaemonFrameError.invalidJSON }
        if let previousSequence, event.sequence <= previousSequence { throw DaemonFrameError.sequenceRegression }
        guard ["ready", "draining", "drained", "fatal"].contains(event.kind) else { throw DaemonFrameError.unknownKind }
        return event
    }
}
