import XCTest
@testable import MembraneTrayMacOS

final class StateTests: XCTestCase {
    func testThreeUnexpectedExitsWithinWindowSurfaceCrashLoop() {
        var reducer = SupervisorReducer(now: Date(timeIntervalSince1970: 0))
        reducer.start(now: Date(timeIntervalSince1970: 0))
        reducer.ready(pid: 7, now: Date(timeIntervalSince1970: 1))
        XCTAssertEqual(reducer.exited(code: 1, signal: nil, now: Date(timeIntervalSince1970: 2)), .backoff)
        reducer.start(now: Date(timeIntervalSince1970: 3)); reducer.ready(pid: 8, now: Date(timeIntervalSince1970: 4))
        XCTAssertEqual(reducer.exited(code: 1, signal: nil, now: Date(timeIntervalSince1970: 5)), .backoff)
        reducer.start(now: Date(timeIntervalSince1970: 6)); reducer.ready(pid: 9, now: Date(timeIntervalSince1970: 7))
        XCTAssertEqual(reducer.exited(code: 1, signal: nil, now: Date(timeIntervalSince1970: 8)), .crashLoop)
        XCTAssertEqual(reducer.observation.reason, "daemon_crash_loop")
    }

    func testLongRunClearsCrashHistory() {
        var reducer = SupervisorReducer(now: Date(timeIntervalSince1970: 0))
        reducer.start(now: Date(timeIntervalSince1970: 0)); reducer.ready(pid: 1, now: Date(timeIntervalSince1970: 1))
        _ = reducer.exited(code: 1, signal: nil, now: Date(timeIntervalSince1970: 2))
        reducer.start(now: Date(timeIntervalSince1970: 3)); reducer.ready(pid: 2, now: Date(timeIntervalSince1970: 64))
        XCTAssertEqual(reducer.crashLoop.failures.count, 0)
    }

    func testManualRestartClearsCrashLoopAndIncrementsGenerationOnce() {
        var reducer = SupervisorReducer(now: Date(timeIntervalSince1970: 0))
        reducer.manualRestart(now: Date(timeIntervalSince1970: 1))
        XCTAssertEqual(reducer.observation.state, .starting)
        XCTAssertEqual(reducer.observation.generation, 1)
        XCTAssertTrue(reducer.crashLoop.failures.isEmpty)
    }

    func testEventCodecRejectsSequenceRegressionAndUnknownKind() throws {
        let json = #"{"schemaVersion":1,"sequence":2,"kind":"ready","pid":9,"observedAtUnixMs":10}"#
        XCTAssertThrowsError(try FrameCodec.decodeEvent(Data((json + "\n").utf8), previousSequence: 2)) { XCTAssertEqual($0 as? DaemonFrameError, .sequenceRegression) }
        let unknown = #"{"schemaVersion":1,"sequence":1,"kind":"other","pid":9,"observedAtUnixMs":10}"#
        XCTAssertThrowsError(try FrameCodec.decodeEvent(Data((unknown + "\n").utf8), previousSequence: nil)) { XCTAssertEqual($0 as? DaemonFrameError, .unknownKind) }
    }
}
