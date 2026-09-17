//  DaemonSupervisor.swift — installed resident-engine holder (post-Architecture-B).
//
//  Retired model (Architecture B): spawned a per-owner `membrane-daemon` child on
//  port 4317 with a stdin launch frame, stdout event stream, and a per-tray
//  launchd kill-guarantee agent. Both lanes are removed: the tray must never own
//  engine lifetime or an independent OS-scheduler lane.
//
//  Current model (shared installed-engine holder, mirroring
//  `apps/membrane-tray-windows/src/installed_holder.rs` + `snapshot.rs`):
//    * Attach to the single installed engine on http://127.0.0.1:47851
//      (`activation::INSTALLED_PORT`; `port` init arg overrides for tests).
//    * Discovery: unsigned `GET /livez` exposes the resident identity tuple
//      (installationId, cortexStoreId, releaseGeneration, startupGeneration,
//      stableInstallRoot). Credential: the engine-published file
//      `<product>/state/tools/.cache/memory/api-token` (canonical 64 lowercase
//      hex). Product root: `~/Library/Application Support/Orthic Labs/Membrane`
//      (`MEMBRANE_PRODUCT_ROOT` overrides for dev/test only).
//    * Transport: canonical loopback-auth V1 — HMAC-SHA256 over the
//      `membrane.loopback.auth.v1` domain-separated canonical frame
//      (membrane-client `residency.rs`), never a bearer header. Requests carry
//      version/nonce/expiry(<=30s)/identity/proof headers plus
//      Host: 127.0.0.1, Content-Length, Connection: close; responses are
//      proof-verified against nonce, status, body SHA-256, and identity.
//    * Holder lease: `POST /resident-holder` with holderKind "hub" (the kind
//      that opens engine background authority), TTL 30s (`expiresAtUnixMs`
//      must stay <= now+60s server-side), renewed every 10s. Renew/release
//      draw the reserved replay partition server-side.
//    * Activation: only when the engine is unreachable, and only by asking the
//      installed client: `<product>/current/membrane activate --install-root
//      <product>/current --timeout-ms 15000`, throttled to one request per 10s.
//    * Release: explicit release on drain/quit; crash or SIGKILL is covered by
//      server-side TTL expiry (<=30s). Final release drains engine background
//      authority and may stop the resident engine — a later owner reactivates.
//
//  Public-surface deltas vs the retired file:
//    * `init(workspaceRoot:daemonPath:dashboardPath:port:)` keeps its labels so
//      TrayUI compiles unchanged, but `workspaceRoot` and `daemonPath` are
//      accepted-and-ignored (no child is ever spawned) and `port` now selects
//      the installed engine endpoint (default 47_851, not the child port 4317).
//    * `observation.pid`, `exitCode`, and `signal` are always nil/omitted — the
//      engine is not a child process and its exit is not ours to observe.
//    * `restart()` re-attaches the holder lease instead of respawning.
//    * `drainAndQuit(completion:)` releases the lease, then completes; quitting
//      via NSApp also releases through `NSApplication.willTerminateNotification`.
//    * `launchDashboard()` is unchanged, but the bootstrap token is now the
//      installed engine credential read from the state dir at launch time.
//    * TrayState meanings: .starting = attach/activation pending, .running =
//      holder lease active, .backoff = lease lost (auto re-attach in flight),
//      .crashLoop = >=3 lease losses in 60s (parked until Restart), .draining =
//      release in flight, .stopped = released or a typed permanent failure.
//      `SupervisorReducer`/`FrameCodec` remain in State.swift for other
//      consumers; this file no longer uses the child-stdout frame protocol.

import AppKit
import Combine
import CryptoKit
import Darwin
import Foundation

@MainActor
public final class DaemonSupervisor: ObservableObject {
    @Published public private(set) var observation: StateObservation

    private var generation = 0
    private var lossTracker = CrashLoopTracker()
    private var worker: InstalledHolderWorker?
    private var workerActive = false
    private var drainCompletion: (() -> Void)?
    private var drainTimeout: DispatchWorkItem?
    private var terminateObserver: NSObjectProtocol?
    private let port: UInt16
    private let productRoot: URL
    private let dashboardPath: URL?

    private var stateRoot: URL { productRoot.appendingPathComponent("state", isDirectory: true) }
    private var stableCurrent: URL { productRoot.appendingPathComponent("current", isDirectory: true) }
    private var activationClient: URL { stableCurrent.appendingPathComponent("membrane") }
    private var endpoint: String { "http://127.0.0.1:\(port)" }

    public init(workspaceRoot: String = FileManager.default.currentDirectoryPath,
                daemonPath: URL? = nil, dashboardPath: URL? = nil, port: UInt16 = 47_851) {
        // `workspaceRoot`/`daemonPath` are retired Architecture-B inputs kept
        // only for source compatibility with TrayUI; see the file header.
        _ = workspaceRoot
        _ = daemonPath
        self.port = port
        self.dashboardPath = dashboardPath
        if let override = ProcessInfo.processInfo.environment["MEMBRANE_PRODUCT_ROOT"],
           !override.trimmingCharacters(in: .whitespaces).isEmpty {
            self.productRoot = URL(fileURLWithPath: override, isDirectory: true)
        } else {
            self.productRoot = FileManager.default.homeDirectoryForCurrentUser
                .appendingPathComponent("Library/Application Support/Orthic Labs/Membrane",
                                        isDirectory: true)
        }
        observation = StateObservation(state: .stopped, generation: 0)
    }

    /// Begin holding the installed resident engine. Idempotent while a worker
    /// generation is active.
    public func start() {
        guard !workerActive else { return }
        generation += 1
        lossTracker.started(at: Date())
        publish(.starting, reason: "engine_attach_pending")
        startWorker()
        if terminateObserver == nil {
            terminateObserver = NotificationCenter.default.addObserver(
                forName: NSApplication.willTerminateNotification, object: nil,
                queue: .main
            ) { [weak self] _ in
                // Runs on the main thread; a synchronous bounded release is
                // wanted before exit, and TTL expiry covers anything left.
                MainActor.assumeIsolated { self?.terminateForAppExit() }
            }
        }
    }

    /// Manual re-attach (Restart button): clears the loss streak, unparks a
    /// crash-looped worker, and nudges an immediate attach attempt.
    public func restart() {
        guard observation.state != .running, observation.state != .draining else { return }
        drainTimeout?.cancel(); drainTimeout = nil
        lossTracker.manualRestart()
        generation += 1
        publish(.starting, reason: "manual_restart")
        if let worker, workerActive {
            worker.unpark()
            worker.nudge()
        } else {
            startWorker()
        }
    }

    /// Release the holder lease, then complete. Mirrors the old signature;
    /// `completion` fires on the main actor.
    public func drainAndQuit(completion: @escaping () -> Void) {
        guard let worker, workerActive else {
            publish(.stopped, reason: "holder_released")
            completion()
            return
        }
        publish(.draining, reason: "holder_release")
        drainCompletion = completion
        worker.requestStop()
        let timeout = DispatchWorkItem { [weak self] in
            Task { @MainActor in
                guard let self, let completion = self.drainCompletion else { return }
                self.drainCompletion = nil
                self.publish(.stopped, reason: "holder_release_timeout")
                completion()
            }
        }
        drainTimeout = timeout
        DispatchQueue.main.asyncAfter(deadline: .now() + 5, execute: timeout)
    }

    /// Dashboard is an on-demand child of the tray, not the engine; it receives
    /// the installed endpoint + credential over stdin, as before.
    public func launchDashboard() {
        guard let dashboardPath else { return }
        let bootstrap = Pipe()
        let process = Process()
        process.executableURL = dashboardPath
        process.standardInput = bootstrap
        do {
            try process.run()
            let token = (try? InstalledEngine.apiToken(stateRoot: stateRoot).get()) ?? ""
            let payload: [String: String] = ["endpoint": endpoint, "token": token]
            var data = try JSONSerialization.data(withJSONObject: payload)
            data.append(10)
            try bootstrap.fileHandleForWriting.write(contentsOf: data)
            try bootstrap.fileHandleForWriting.close()
        } catch { /* dashboard is on-demand; tray remains healthy */ }
    }

    // MARK: - worker lifecycle

    private func startWorker() {
        let worker = InstalledHolderWorker(
            port: port,
            stateRoot: stateRoot,
            stableCurrent: stableCurrent,
            activationClient: activationClient
        ) { [weak self] event in
            Task { @MainActor in self?.handle(event) }
        }
        self.worker = worker
        workerActive = true
        worker.start()
    }

    /// willTerminate path: stop and synchronously wait (bounded) so the lease
    /// is released before exit when possible. TTL expiry is the fallback.
    private func terminateForAppExit() {
        if let terminateObserver {
            NotificationCenter.default.removeObserver(terminateObserver)
            terminateObserver = nil
        }
        drainTimeout?.cancel(); drainTimeout = nil
        guard let worker, workerActive else { return }
        worker.requestStop()
        _ = worker.join(deadline: .now() + 2.5)
        drainCompletion = nil
        publish(.stopped, reason: "holder_released")
    }

    private func handle(_ event: InstalledHolderWorker.Event) {
        switch event {
        case .unreachable(let reason):
            // Engine not answering while attaching: keep .starting, update the
            // typed reason without churning identical publications.
            guard observation.state == .starting, observation.reason != reason else { return }
            publish(.starting, reason: reason)
        case .acquired:
            // An acquire ack racing a drain must not flash "running".
            guard observation.state != .draining else { return }
            lossTracker.started(at: Date())
            publish(.running)
        case .lost(let reason):
            guard observation.state == .running else { return }
            if lossTracker.unexpectedExit(at: Date()) {
                publish(.crashLoop, reason: "holder_lease_crash_loop")
                worker?.park()
            } else {
                publish(.backoff, reason: reason)
            }
        case .unavailable(let reason):
            // Permanent-typed failure (missing install, credential migration):
            // park until Restart rather than spin.
            guard observation.state != .draining else { return }
            publish(.stopped, reason: reason)
        case .exited:
            workerActive = false
            drainTimeout?.cancel(); drainTimeout = nil
            if let completion = drainCompletion {
                drainCompletion = nil
                publish(.stopped, reason: "holder_released")
                completion()
            } else if observation.state != .stopped {
                publish(.stopped, reason: "holder_worker_exited")
            }
        }
    }

    private func publish(_ state: TrayState, reason: String? = nil) {
        observation = StateObservation(state: state, reason: reason,
                                       generation: generation, observedAt: Date())
    }
}

// MARK: - Holder worker

/// Off-actor holder loop, one instance per attach generation. All network I/O
/// is bounded-blocking loopback on a utility queue; the main actor owns every
/// state publication via `onEvent`.
private final class InstalledHolderWorker: @unchecked Sendable {
    enum Event: Sendable {
        /// Attach attempt failed while unheld; reason is a typed snake_case tag.
        case unreachable(String)
        /// Acquire acknowledged by the fenced controller (startupGeneration).
        case acquired(UInt64)
        /// Previously held lease lost (renew rejected, controller rotated,
        /// engine gone). The worker re-enters the attach loop automatically.
        case lost(String)
        /// Permanent-typed failure; worker parks until `unpark()`.
        case unavailable(String)
        /// Worker finished; release was already attempted if a lease was held.
        case exited
    }

    private let port: UInt16
    private let stateRoot: URL
    private let stableCurrent: URL
    private let activationClient: URL
    private let onEvent: @Sendable (Event) -> Void
    private let holder: ResidentHolderCredential

    private let lock = NSLock()
    private var stopFlag = false
    private var parked = false
    private let wake = DispatchSemaphore(value: 0)
    private let finished = DispatchSemaphore(value: 0)

    private static let ttlMs: UInt64 = 30_000
    private static let renewInterval: TimeInterval = 10
    private static let attachPoll: TimeInterval = 0.5
    private static let activationCooldown: TimeInterval = 10

    init(port: UInt16, stateRoot: URL, stableCurrent: URL, activationClient: URL,
         onEvent: @escaping @Sendable (Event) -> Void) {
        self.port = port
        self.stateRoot = stateRoot
        self.stableCurrent = stableCurrent
        self.activationClient = activationClient
        self.onEvent = onEvent
        let pid = getpid()
        self.holder = ResidentHolderCredential(
            holderKind: "hub",
            holderId: "tray-macos-\(pid)",
            credentialId: "tray-macos-\(pid)-credential")
    }

    func start() {
        DispatchQueue(label: "membrane-tray.holder", qos: .utility).async { self.run() }
    }

    func requestStop() { lock.lock(); stopFlag = true; lock.unlock(); wake.signal() }
    func park() { lock.lock(); parked = true; lock.unlock() }
    func unpark() { lock.lock(); parked = false; lock.unlock(); wake.signal() }
    func nudge() { wake.signal() }
    func join(deadline: DispatchTime) -> Bool { finished.wait(timeout: deadline) == .success }

    private var stopped: Bool { lock.lock(); defer { lock.unlock() }; return stopFlag }
    private var isParked: Bool { lock.lock(); defer { lock.unlock() }; return parked }
    private func setParked(_ value: Bool) { lock.lock(); parked = value; lock.unlock() }

    private enum Attach {
        case observed(ControllerIdentity)
        case retry(String)        // transient: unreachable, timeout, protocol fault
        case unavailable(String)  // permanent until a manual restart
    }

    private func run() {
        var controller: ControllerIdentity? = nil
        // Mirrors the reference worker: activation requests are gated to one
        // per cooldown starting *now*, so a just-starting engine gets ~10s to
        // publish before the first `activate` request fires.
        var lastActivation = Date()
        attach: while !stopped {
            if isParked {
                _ = wake.wait(timeout: .distantFuture)
                continue
            }
            if controller == nil {
                switch fetchController() {
                case .observed(let identity):
                    if stopped { break attach }
                    let acquire = HolderRequest.acquire(controller: identity, holder: holder,
                                                        ttlMs: Self.ttlMs)
                    if let response = dispatch(acquire),
                       response.controller == acquire.controller,
                       response.status.controllerActive {
                        onEvent(.acquired(identity.startupGeneration))
                        if stopped {
                            _ = dispatch(.release(controller: acquire.controller,
                                                  holder: holder))
                            break attach
                        }
                        controller = identity
                        continue
                    }
                    // Acquire rejected (draining controller, identity race):
                    // retry on the attach cadence, no activation needed.
                case .retry(let reason):
                    onEvent(.unreachable(reason))
                    if !stopped, lastActivation.timeIntervalSinceNow <= -Self.activationCooldown {
                        // Cooldown covers missing-client outcomes too, so the
                        // attach poll cannot spin on activation attempts.
                        _ = requestActivation()
                        lastActivation = Date()
                    }
                case .unavailable(let reason):
                    setParked(true)
                    onEvent(.unavailable(reason))
                    continue
                }
                _ = wake.wait(timeout: .now() + Self.attachPoll)
                continue
            }
            // Held: renew well inside the 30s TTL.
            _ = wake.wait(timeout: .now() + Self.renewInterval)
            if stopped { break }
            if isParked { continue }
            guard let previous = controller else { continue }
            switch fetchController() {
            case .observed(let observed):
                if observed != previous {
                    controller = nil
                    onEvent(.lost("resident_controller_mismatch"))
                    continue
                }
                let renew = HolderRequest.renew(controller: previous, holder: holder,
                                                ttlMs: Self.ttlMs)
                if let response = dispatch(renew),
                   response.controller == previous,
                   response.status.controllerActive {
                    continue
                }
                controller = nil
                onEvent(.lost("resident_holder_renew_failed"))
            case .retry(let reason):
                controller = nil
                onEvent(.lost(reason))
            case .unavailable(let reason):
                controller = nil
                setParked(true)
                onEvent(.unavailable(reason))
            }
        }
        if let controller {
            _ = dispatch(.release(controller: controller, holder: holder))
        }
        onEvent(.exited)
        finished.signal()
    }

    // MARK: authenticated status + lease dispatch

    /// The "authenticated status" query: unsigned /livez identity discovery,
    /// signed /health fence, then a signed Status operation against that same
    /// fenced controller. Never creates holder authority itself.
    private func fetchController() -> Attach {
        var isDirectory: ObjCBool = false
        guard FileManager.default.fileExists(atPath: stableCurrent.path,
                                             isDirectory: &isDirectory),
              isDirectory.boolValue else {
            return .unavailable("installed_layout_missing")
        }
        let token: String
        switch InstalledEngine.apiToken(stateRoot: stateRoot) {
        case .success(let value): token = value
        case .failure(let failure):
            switch failure.reason {
            case "workspace_api_token_env_migration_required",
                 "workspace_api_token_migration_required":
                return .unavailable(failure.reason)
            default:
                return .retry(failure.reason)
            }
        }
        do {
            let identity = try InstalledEngine.discoverIdentity(port: port)
            let observed = try InstalledEngine.healthController(port: port, token: token,
                                                                identity: identity)
            let statusRequest = HolderRequest.status(controller: observed)
            guard let response = dispatch(statusRequest),
                  response.schemaVersion == ResidentHolderContract.schemaVersion,
                  response.operation == "status",
                  response.controller == observed else {
                return .retry("resident_holder_status_unavailable")
            }
            return .observed(observed)
        } catch let error as InstalledEngine.Failure {
            return .retry(error.reason)
        } catch {
            return .retry("engine_unreachable")
        }
    }

    /// One signed POST /resident-holder. Returns nil on any transport,
    /// authentication, or rejection outcome — callers map nil to their own
    /// retry/lost semantics exactly as the Windows reference does.
    private func dispatch(_ request: HolderRequest) -> HolderResponse? {
        guard let body = try? JSONEncoder().encode(request) else { return nil }
        let token: String
        switch InstalledEngine.apiToken(stateRoot: stateRoot) {
        case .success(let value): token = value
        case .failure: return nil
        }
        do {
            let identity = try InstalledEngine.discoverIdentity(port: port)
            let response = try InstalledEngine.signedExchange(
                port: port, token: token, identity: identity,
                method: "POST", target: "/resident-holder",
                contentType: "application/json", body: body)
            guard response.status == 200 else { return nil }
            return try? JSONDecoder().decode(HolderResponse.self, from: response.body)
        } catch { return nil }
    }

    /// Activation is a request to installed client/OS supervision only: the
    /// tray never spawns, supervises, or stops the engine itself.
    private func requestActivation() -> Bool {
        var isDirectory: ObjCBool = false
        guard FileManager.default.fileExists(atPath: activationClient.path),
              FileManager.default.fileExists(atPath: stableCurrent.path,
                                             isDirectory: &isDirectory),
              isDirectory.boolValue else { return false }
        let process = Process()
        process.executableURL = activationClient
        process.arguments = ["activate", "--install-root", stableCurrent.path,
                             "--timeout-ms", "15000"]
        process.standardInput = FileHandle.nullDevice
        process.standardOutput = FileHandle.nullDevice
        process.standardError = FileHandle.nullDevice
        return (try? process.run()) != nil
    }
}

// MARK: - Wire types

private enum ResidentHolderContract {
    static let schemaVersion: UInt32 = 1
}

/// Wire shape of `ResidentControllerIdentityV1` (camelCase, closed).
private struct ControllerIdentity: Codable, Equatable, Sendable {
    var installationId: String
    var cortexStoreId: String
    var releaseGeneration: String
    var startupGeneration: UInt64
    var stableCurrent: String
}

/// Wire shape of `ResidentHolderCredentialV1`.
private struct ResidentHolderCredential: Codable, Equatable, Sendable {
    var holderKind: String
    var holderId: String
    var credentialId: String
}

/// Wire shape of `ResidentHolderRequestV1`. Optionals are omitted when nil so
/// the server-side `deny_unknown_fields` decoder never sees a `lossCursor`.
private struct HolderRequest: Encodable {
    var schemaVersion: UInt32 = ResidentHolderContract.schemaVersion
    var operation: String
    var controller: ControllerIdentity
    var holder: ResidentHolderCredential?
    var expiresAtUnixMs: UInt64?
    var observedAtUnixMs: UInt64

    static func acquire(controller: ControllerIdentity,
                        holder: ResidentHolderCredential, ttlMs: UInt64) -> Self {
        make(operation: "acquire", controller: controller, holder: holder, ttlMs: ttlMs)
    }
    static func renew(controller: ControllerIdentity,
                      holder: ResidentHolderCredential, ttlMs: UInt64) -> Self {
        make(operation: "renew", controller: controller, holder: holder, ttlMs: ttlMs)
    }
    static func release(controller: ControllerIdentity,
                        holder: ResidentHolderCredential) -> Self {
        make(operation: "release", controller: controller, holder: holder, ttlMs: 0)
    }
    static func status(controller: ControllerIdentity) -> Self {
        make(operation: "status", controller: controller, holder: nil, ttlMs: 0)
    }
    private static func make(operation: String, controller: ControllerIdentity,
                             holder: ResidentHolderCredential?, ttlMs: UInt64) -> Self {
        let now = Self.nowMs()
        return HolderRequest(operation: operation, controller: controller, holder: holder,
                             expiresAtUnixMs: ttlMs == 0 ? nil : now &+ ttlMs,
                             observedAtUnixMs: now)
    }
    private static func nowMs() -> UInt64 {
        UInt64(Date().timeIntervalSince1970 * 1000)
    }
}

/// Wire shape of `ResidentHolderResponseV1` (loss/status extras tolerated).
private struct HolderResponse: Decodable {
    var schemaVersion: UInt32
    var operation: String
    var controller: ControllerIdentity
    var status: HolderStatus
}

private struct HolderStatus: Decodable {
    var controllerActive: Bool
}

// MARK: - Installed engine transport (canonical loopback-auth V1)

/// Port of `membrane-client::residency` loopback-auth V1 to CryptoKit: the same
/// domain string, field order, u32be-length-prefixed encoding, header profile,
/// nonce/expiry bounds, and response proof verification. Nothing here invents
/// an alternate transport — unsigned /livez is the only bearer-free exchange,
/// matching the engine's explicit exemption.
private enum InstalledEngine {
    struct Failure: Error { let reason: String }

    struct Identity: Equatable {
        var installationId: String
        var cortexStoreId: String
        var releaseGeneration: String
        var startupGeneration: UInt64
        var stableInstallRoot: String
    }

    struct Response {
        var status: UInt16
        var headers: [(String, String)]
        var body: Data
    }

    private static let domain = "membrane.loopback.auth.v1"
    private static let host = "127.0.0.1"
    private static let nonceOctets = 32
    private static let maxExpirySecs: UInt64 = 30
    private static let requestTimeout: TimeInterval = 0.5
    private static let maxResponseBytes = 1_052_672 // 1 MiB + 4096, as snapshot.rs
    private static let maxHeaderBytes = 16 * 1024

    private static let semanticHeaders: Set<String> = [
        "x-membrane-loopback-version",
        "x-membrane-loopback-nonce",
        "x-membrane-loopback-expiry",
        "x-membrane-loopback-installation-id",
        "x-membrane-loopback-cortex-store-id",
        "x-membrane-loopback-release-generation",
        "x-membrane-loopback-startup-generation",
        "x-membrane-loopback-stable-install-root",
        "x-membrane-loopback-proof",
    ]
    private static let transportHeaders: Set<String> = [
        "host", "content-length", "connection", "content-type",
    ]

    // MARK: credential resolution (mirrors workspace.rs `api_token`)

    /// Read-only resolver for the engine-published credential. The tray never
    /// writes, generates, or replaces the token; `serve.rs` is the sole
    /// authority. Typed outcomes mirror the Rust resolver.
    static func apiToken(stateRoot: URL) -> Result<String, Failure> {
        if ProcessInfo.processInfo.environment["MEMBRANE_API_TOKEN"] != nil
            || ProcessInfo.processInfo.environment["MEMBRANE_API_TOKEN_FILE"] != nil {
            return .failure(Failure(reason: "workspace_api_token_env_migration_required"))
        }
        let path = stateRoot.appendingPathComponent("tools/.cache/memory/api-token")
        var info = Darwin.stat()
        let status = path.path.withCString { lstat($0, &info) }
        guard status == 0 else {
            return .failure(Failure(reason: errno == ENOENT
                                    ? "workspace_api_token_pending_publication"
                                    : "workspace_api_token_unreadable"))
        }
        let kind = info.st_mode & 0o170000
        guard kind == 0o100000 else { // regular file only; symlinks rejected
            return .failure(Failure(reason: "workspace_api_token_invalid"))
        }
        guard let raw = try? String(contentsOf: path, encoding: .utf8) else {
            return .failure(Failure(reason: "workspace_api_token_unreadable"))
        }
        var token = raw
        while token.last == "\r" || token.last == "\n" { token.removeLast() }
        guard token.count == 64,
              token.allSatisfy({ $0.isASCII && ($0.isNumber || ("a"..."f").contains($0)) }) else {
            return .failure(Failure(reason: "workspace_api_token_migration_required"))
        }
        return .success(token)
    }

    // MARK: discovery + health fence

    /// Unsigned `GET /livez`: the engine's explicit exemption returns the
    /// resident identity hints every signed request then proves.
    static func discoverIdentity(port: UInt16) throws -> Identity {
        let wire = Data("GET /livez HTTP/1.1\r\nHost: \(host)\r\nConnection: close\r\n\r\n".utf8)
        let response = try exchange(port: port, wire: wire)
        guard response.status == 200 || response.status == 503 else {
            throw Failure(reason: "resident_health_unavailable")
        }
        guard let object = try? JSONSerialization.jsonObject(with: response.body),
              let health = object as? [String: Any] else {
            throw Failure(reason: "resident_health_invalid")
        }
        return try identityFields(from: health)
    }

    /// Signed `GET /health`: proves the loopback credential, then yields the
    /// fenced controller identity used by every holder operation.
    static func healthController(port: UInt16, token: String,
                                 identity: Identity) throws -> ControllerIdentity {
        let response = try signedExchange(port: port, token: token, identity: identity,
                                          method: "GET", target: "/health",
                                          contentType: "", body: Data())
        guard response.status == 200 || response.status == 503 else {
            throw Failure(reason: "resident_health_unavailable")
        }
        guard let object = try? JSONSerialization.jsonObject(with: response.body),
              let health = object as? [String: Any] else {
            throw Failure(reason: "resident_health_invalid")
        }
        guard health["serviceId"] as? String == "membrane-hub",
              health["runtimeOrigin"] as? String == "installed",
              health["nativeOnly"] as? Bool == true else {
            throw Failure(reason: "resident_health_identity_invalid")
        }
        let hinted = try identityFields(from: health)
        if response.status == 200, (health["ok"] as? Bool) != true {
            throw Failure(reason: "resident_health_unavailable")
        }
        return ControllerIdentity(installationId: hinted.installationId,
                                  cortexStoreId: hinted.cortexStoreId,
                                  releaseGeneration: hinted.releaseGeneration,
                                  startupGeneration: hinted.startupGeneration,
                                  stableCurrent: hinted.stableInstallRoot)
    }

    private static func identityFields(from health: [String: Any]) throws -> Identity {
        func field(_ name: String) throws -> String {
            guard let value = (health[name] as? String)?
                    .trimmingCharacters(in: .whitespaces), !value.isEmpty else {
                throw Failure(reason: "resident_health_identity_invalid")
            }
            return value
        }
        let startup = (health["startupGeneration"] as? NSNumber)?.uint64Value
        guard let startup, startup != 0 else {
            throw Failure(reason: "resident_health_identity_invalid")
        }
        return try Identity(installationId: field("installationId"),
                            cortexStoreId: field("cortexStoreId"),
                            releaseGeneration: field("releaseGeneration"),
                            startupGeneration: startup,
                            stableInstallRoot: field("stableInstallRoot"))
    }

    // MARK: signed exchange

    static func signedExchange(port: UInt16, token: String, identity: Identity,
                               method: String, target: String,
                               contentType: String, body: Data) throws -> Response {
        guard token.count == 64,
              let key = hexDecode(token, bytes: 32) else {
            throw Failure(reason: "snapshot_auth_invalid")
        }
        let now = UInt64(Date().timeIntervalSince1970)
        let expiry = now &+ min(10, maxExpirySecs)
        var nonce = [UInt8](repeating: 0, count: nonceOctets)
        for index in nonce.indices { nonce[index] = UInt8.random(in: 0...255) }
        let proof = requestProof(key: key, method: method, target: target,
                                 contentType: contentType, body: body,
                                 identity: identity, nonce: nonce, expiry: expiry)
        var head = "\(method) \(target) HTTP/1.1\r\n"
        let headers: [(String, String)] = [
            ("x-membrane-loopback-version", "1"),
            ("x-membrane-loopback-nonce", hexEncode(nonce)),
            ("x-membrane-loopback-expiry", String(expiry)),
            ("x-membrane-loopback-installation-id", identity.installationId),
            ("x-membrane-loopback-cortex-store-id", identity.cortexStoreId),
            ("x-membrane-loopback-release-generation", identity.releaseGeneration),
            ("x-membrane-loopback-startup-generation", String(identity.startupGeneration)),
            ("x-membrane-loopback-stable-install-root", identity.stableInstallRoot),
            ("x-membrane-loopback-proof", hexEncode(proof)),
            ("host", host),
            ("content-length", String(body.count)),
            ("connection", "close"),
        ]
        for (name, value) in headers { head += "\(name): \(value)\r\n" }
        if !contentType.isEmpty { head += "content-type: \(contentType)\r\n" }
        head += "\r\n"
        var wire = Data(head.utf8)
        wire.append(body)
        let response = try exchange(port: port, wire: wire)
        try verify(response: response, key: key, nonce: nonce, expiry: expiry,
                   identity: identity)
        return response
    }

    private static func verify(response: Response, key: Data, nonce: [UInt8],
                               expiry: UInt64, identity: Identity) throws {
        // Canonical profile: exactly one of each semantic header, transport
        // headers only, no Authorization.
        var seen = Set<String>()
        for (name, _) in response.headers {
            guard semanticHeaders.contains(name) || transportHeaders.contains(name) else {
                throw Failure(reason: "response_auth_invalid")
            }
            if semanticHeaders.contains(name), !seen.insert(name).inserted {
                throw Failure(reason: "response_auth_invalid")
            }
        }
        guard seen.count == semanticHeaders.count else {
            throw Failure(reason: "response_auth_invalid")
        }
        func header(_ name: String) throws -> String {
            guard let value = response.headers.first(where: { $0.0 == name })?.1 else {
                throw Failure(reason: "response_auth_invalid")
            }
            return value
        }
        let connection = try header("connection")
        let version = try header("x-membrane-loopback-version")
        let expiryHeader = try header("x-membrane-loopback-expiry")
        let lengthHeader = try header("content-length")
        let nonceHeader = try header("x-membrane-loopback-nonce")
        let installationId = try header("x-membrane-loopback-installation-id")
        let cortexStoreId = try header("x-membrane-loopback-cortex-store-id")
        let releaseGeneration = try header("x-membrane-loopback-release-generation")
        let startupGeneration = try header("x-membrane-loopback-startup-generation")
        let stableInstallRoot = try header("x-membrane-loopback-stable-install-root")
        guard connection == "close",
              version == "1",
              expiryHeader == String(expiry),
              expiry > UInt64(Date().timeIntervalSince1970),
              Int(lengthHeader) == response.body.count,
              nonceHeader == hexEncode(nonce),
              installationId == identity.installationId,
              cortexStoreId == identity.cortexStoreId,
              releaseGeneration == identity.releaseGeneration,
              startupGeneration == String(identity.startupGeneration),
              stableInstallRoot == identity.stableInstallRoot else {
            throw Failure(reason: "response_auth_invalid")
        }
        let proofHeader = try header("x-membrane-loopback-proof")
        guard let proof = hexDecode(proofHeader, bytes: 32) else {
            throw Failure(reason: "response_auth_invalid")
        }
        let tag = responseTag(key: key, nonce: nonce, status: response.status,
                              body: response.body, identity: identity)
        guard proof == Data(tag) else {
            throw Failure(reason: "response_auth_invalid")
        }
    }

    // MARK: canonical frames (membrane-client residency.rs)

    private static func requestProof(key: Data, method: String, target: String,
                                     contentType: String, body: Data,
                                     identity: Identity, nonce: [UInt8],
                                     expiry: UInt64) -> [UInt8] {
        var buffer = [UInt8]()
        pushPrefix(&buffer, kind: 1)
        pushField(&buffer, [UInt8](method.utf8))
        pushField(&buffer, [UInt8](target.utf8))
        pushField(&buffer, [UInt8](host.utf8))
        pushField(&buffer, [UInt8](contentType.utf8))
        pushField(&buffer, [UInt8](SHA256.hash(data: body)))
        pushField(&buffer, [UInt8](identity.installationId.utf8))
        pushField(&buffer, [UInt8](identity.cortexStoreId.utf8))
        pushField(&buffer, [UInt8](identity.releaseGeneration.utf8))
        pushUInt64(&buffer, identity.startupGeneration)
        pushField(&buffer, [UInt8](identity.stableInstallRoot.utf8))
        pushField(&buffer, nonce)
        pushUInt64(&buffer, expiry)
        return [UInt8](HMAC<SHA256>.authenticationCode(for: Data(buffer),
                                                       using: SymmetricKey(data: key)))
    }

    private static func responseTag(key: Data, nonce: [UInt8], status: UInt16,
                                    body: Data, identity: Identity) -> [UInt8] {
        var buffer = [UInt8]()
        pushPrefix(&buffer, kind: 2)
        pushField(&buffer, nonce)
        buffer.append(UInt8(status >> 8))
        buffer.append(UInt8(status & 0xff))
        pushField(&buffer, [UInt8](SHA256.hash(data: body)))
        pushField(&buffer, [UInt8](identity.installationId.utf8))
        pushField(&buffer, [UInt8](identity.cortexStoreId.utf8))
        pushField(&buffer, [UInt8](identity.releaseGeneration.utf8))
        pushUInt64(&buffer, identity.startupGeneration)
        pushField(&buffer, [UInt8](identity.stableInstallRoot.utf8))
        return [UInt8](HMAC<SHA256>.authenticationCode(for: Data(buffer),
                                                       using: SymmetricKey(data: key)))
    }

    private static func pushPrefix(_ buffer: inout [UInt8], kind: UInt8) {
        pushField(&buffer, [UInt8](domain.utf8))
        buffer.append(kind)
    }

    private static func pushField(_ buffer: inout [UInt8], _ octets: [UInt8]) {
        pushUInt32(&buffer, UInt32(octets.count))
        buffer.append(contentsOf: octets)
    }

    private static func pushUInt32(_ buffer: inout [UInt8], _ value: UInt32) {
        buffer.append(UInt8((value >> 24) & 0xff))
        buffer.append(UInt8((value >> 16) & 0xff))
        buffer.append(UInt8((value >> 8) & 0xff))
        buffer.append(UInt8(value & 0xff))
    }

    private static func pushUInt64(_ buffer: inout [UInt8], _ value: UInt64) {
        for shift in stride(from: 56, through: 0, by: -8) {
            buffer.append(UInt8((value >> UInt64(shift)) & 0xff))
        }
    }

    private static func hexEncode(_ bytes: [UInt8]) -> String {
        bytes.map { String(format: "%02x", $0) }.joined()
    }

    private static func hexDecode(_ value: String, bytes count: Int) -> Data? {
        guard value.count == count * 2,
              value.allSatisfy({ $0.isASCII && $0.isHexDigit && !$0.isUppercase }) else {
            return nil
        }
        var out = Data(capacity: count)
        var index = value.startIndex
        while index < value.endIndex {
            let next = value.index(index, offsetBy: 2)
            guard let byte = UInt8(value[index..<next], radix: 16) else { return nil }
            out.append(byte)
            index = next
        }
        return out
    }

    // MARK: bounded-blocking HTTP/1.1 loopback exchange

    private enum Transport: Error { case unavailable, timeout, invalid, tooLarge }

    private static func exchange(port: UInt16, wire: Data) throws -> Response {
        do {
            return try exchangeOrThrow(port: port, wire: wire)
        } catch let error as Transport {
            switch error {
            case .timeout: throw Failure(reason: "snapshot_timeout")
            case .tooLarge: throw Failure(reason: "snapshot_too_large")
            default: throw Failure(reason: "engine_unreachable")
            }
        }
    }

    private static func exchangeOrThrow(port: UInt16, wire: Data) throws -> Response {
        let deadline = Date().addingTimeInterval(requestTimeout)
        let descriptor = socket(AF_INET, SOCK_STREAM, 0)
        guard descriptor >= 0 else { throw Transport.unavailable }
        defer { close(descriptor) }
        var one: Int32 = 1
        _ = setsockopt(descriptor, SOL_SOCKET, SO_NOSIGPIPE, &one,
                       socklen_t(MemoryLayout<Int32>.size))
        let flags = fcntl(descriptor, F_GETFL, 0)
        _ = fcntl(descriptor, F_SETFL, flags | O_NONBLOCK)
        var address = sockaddr_in()
        address.sin_len = UInt8(MemoryLayout<sockaddr_in>.size)
        address.sin_family = sa_family_t(AF_INET)
        address.sin_port = port.bigEndian
        address.sin_addr = in_addr(s_addr: in_addr_t(0x7F000001).bigEndian)
        let connected = withUnsafePointer(to: &address) { pointer in
            pointer.withMemoryRebound(to: sockaddr.self, capacity: 1) {
                connect(descriptor, $0, socklen_t(MemoryLayout<sockaddr_in>.size))
            }
        }
        if connected != 0 {
            guard errno == EINPROGRESS else { throw Transport.unavailable }
            try wait(for: descriptor, event: Int16(POLLOUT), deadline: deadline)
            var code: Int32 = 0
            var length = socklen_t(MemoryLayout<Int32>.size)
            guard getsockopt(descriptor, SOL_SOCKET, SO_ERROR, &code, &length) == 0,
                  code == 0 else { throw Transport.unavailable }
        }
        _ = fcntl(descriptor, F_SETFL, flags)
        try wire.withUnsafeBytes { pointer in
            guard let base = pointer.baseAddress else { return }
            var sent = 0
            while sent < wire.count {
                try wait(for: descriptor, event: Int16(POLLOUT), deadline: deadline)
                let count = send(descriptor, base + sent, wire.count - sent, 0)
                guard count > 0 else { throw Transport.unavailable }
                sent += count
            }
        }
        var raw = Data()
        var headerEnd: Data.Index? = nil
        while headerEnd == nil {
            if raw.count > maxHeaderBytes { throw Transport.tooLarge }
            let chunk = try read(descriptor, deadline: deadline, limit: 8192)
            guard !chunk.isEmpty else { throw Transport.invalid } // EOF pre-headers
            raw.append(contentsOf: chunk)
            headerEnd = raw.range(of: Data([13, 10, 13, 10]))?.lowerBound
        }
        guard let split = headerEnd,
              let text = String(data: raw[..<split], encoding: .utf8) else {
            throw Transport.invalid
        }
        var lines = text.components(separatedBy: "\r\n")
        let statusLine = lines.removeFirst().split(separator: " ")
        guard statusLine.count >= 2, statusLine[0] == "HTTP/1.1",
              let status = UInt16(statusLine[1]) else {
            throw Transport.invalid
        }
        var headers = [(String, String)]()
        var contentLength: Int? = nil
        for line in lines where !line.isEmpty {
            guard let colon = line.firstIndex(of: ":") else { throw Transport.invalid }
            let name = String(line[..<colon]).lowercased()
            let value = String(line[line.index(after: colon)...])
                .trimmingCharacters(in: CharacterSet(charactersIn: " \t"))
            guard name != "transfer-encoding" else { throw Transport.invalid }
            if name == "content-length" {
                guard contentLength == nil, let parsed = Int(value) else {
                    throw Transport.invalid
                }
                contentLength = parsed
            }
            headers.append((name, value))
        }
        guard let length = contentLength else { throw Transport.invalid }
        guard length <= maxResponseBytes else { throw Transport.tooLarge }
        // Body begins immediately after the 4-byte separator.
        let bodyStart = raw.index(split, offsetBy: 4)
        while raw.count - bodyStart < length {
            let chunk = try read(descriptor, deadline: deadline,
                                 limit: min(8192, length - (raw.count - bodyStart)))
            guard !chunk.isEmpty else { throw Transport.invalid }
            raw.append(contentsOf: chunk)
            if raw.count > maxHeaderBytes + maxResponseBytes { throw Transport.tooLarge }
        }
        guard raw.count - bodyStart == length else { throw Transport.invalid }
        return Response(status: status, headers: headers,
                        body: raw[bodyStart...])
    }

    private static func wait(for descriptor: Int32, event: Int16,
                             deadline: Date) throws {
        let remainingMs = deadline.timeIntervalSinceNow * 1000
        guard remainingMs > 0 else { throw Transport.timeout }
        var poller = pollfd(fd: descriptor, events: event, revents: 0)
        let ready = poll(&poller, 1, Int32(min(remainingMs, Double(Int32.max))))
        guard ready > 0 else {
            if ready == 0 { throw Transport.timeout }
            throw Transport.unavailable
        }
    }

    private static func read(_ descriptor: Int32, deadline: Date,
                             limit: Int) throws -> [UInt8] {
        try wait(for: descriptor, event: Int16(POLLIN), deadline: deadline)
        var chunk = [UInt8](repeating: 0, count: max(1, limit))
        let count = chunk.withUnsafeMutableBytes { pointer in
            recv(descriptor, pointer.baseAddress, pointer.count, 0)
        }
        if count < 0 { throw Transport.unavailable }
        return Array(chunk.prefix(count))
    }
}
