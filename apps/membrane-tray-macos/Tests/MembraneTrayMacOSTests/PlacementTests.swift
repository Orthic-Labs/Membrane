import XCTest
@testable import MembraneTrayMacOS

final class PlacementTests: XCTestCase {
    let work = TrayRect(left: 0, top: 0, right: 1920, bottom: 1040)
    let panel = TraySize(width: 340, height: 500)

    func testBottomMenuBarChoosesAboveWhenBelowDoesNotFit() {
        let origin = popoverOrigin(anchor: TrayRect(left: 900, top: 980, right: 920, bottom: 1000), size: panel, workArea: work, edge: .bottom)
        XCTAssertEqual(origin.y, 480)
    }

    func testTopAndSideBarsRemainWithinWorkArea() {
        XCTAssertEqual(popoverOrigin(anchor: TrayRect(left: 900, top: 20, right: 920, bottom: 40), size: panel, workArea: work, edge: .top).y, 40)
        XCTAssertEqual(popoverOrigin(anchor: TrayRect(left: 0, top: 500, right: 20, bottom: 520), size: panel, workArea: work, edge: .left).x, 20)
        XCTAssertEqual(popoverOrigin(anchor: TrayRect(left: 1900, top: 500, right: 1920, bottom: 520), size: panel, workArea: work, edge: .right).x, 1560)
    }

    func testBlurGraceAndPointerGestureGuard() {
        var guardState = DismissGuard(); guardState.trayClick(at: 1_000)
        XCTAssertFalse(guardState.shouldDismiss(at: 1_499, focusLost: true)); XCTAssertTrue(guardState.shouldDismiss(at: 1_500, focusLost: true))
        guardState.trayClick(at: 2_000); guardState.pointerDown()
        XCTAssertFalse(guardState.shouldDismiss(at: 3_000, focusLost: true)); guardState.pointerUpOrCancel()
        XCTAssertTrue(guardState.shouldDismiss(at: 3_000, focusLost: true)); XCTAssertFalse(guardState.shouldDismiss(at: 3_000, focusLost: false))
    }
}
