import Foundation

public struct TrayRect: Equatable, Sendable {
    public var left: Double; public var top: Double; public var right: Double; public var bottom: Double
    public init(left: Double, top: Double, right: Double, bottom: Double) { self.left = left; self.top = top; self.right = right; self.bottom = bottom }
}
public struct TraySize: Equatable, Sendable { public var width: Double; public var height: Double; public init(width: Double, height: Double) { self.width = width; self.height = height } }
public struct TrayPoint: Equatable, Sendable { public var x: Double; public var y: Double; public init(x: Double, y: Double) { self.x = x; self.y = y } }
public enum TrayEdge: Sendable { case top, bottom, left, right }

public func popoverOrigin(anchor: TrayRect, size: TraySize, workArea: TrayRect, edge: TrayEdge) -> TrayPoint {
    let clampX: (Double) -> Double = { min(max($0, workArea.left), workArea.right - size.width) }
    let clampY: (Double) -> Double = { min(max($0, workArea.top), workArea.bottom - size.height) }
    let centeredX = anchor.left + (anchor.right - anchor.left - size.width) / 2
    let centeredY = anchor.top + (anchor.bottom - anchor.top - size.height) / 2
    switch edge {
    case .top: return TrayPoint(x: clampX(centeredX), y: clampY(anchor.bottom))
    case .left: return TrayPoint(x: clampX(anchor.right), y: clampY(centeredY))
    case .right: return TrayPoint(x: clampX(anchor.left - size.width), y: clampY(centeredY))
    case .bottom:
        let below = anchor.bottom, above = anchor.top - size.height
        let y: Double
        if below + size.height <= workArea.bottom { y = below }
        else if above >= workArea.top { y = above }
        else if workArea.bottom - below >= anchor.top - workArea.top { y = workArea.bottom - size.height }
        else { y = workArea.top }
        return TrayPoint(x: clampX(centeredX), y: clampY(y))
    }
}

public struct DismissGuard: Sendable {
    private var clickedAt: UInt64?
    private var gesture = false
    public init() {}
    public mutating func trayClick(at milliseconds: UInt64) { clickedAt = milliseconds; gesture = false }
    public mutating func pointerDown() { gesture = true }
    public mutating func pointerUpOrCancel() { gesture = false }
    public func shouldDismiss(at milliseconds: UInt64, focusLost: Bool) -> Bool {
        guard focusLost, !gesture else { return false }
        guard let clickedAt else { return true }
        return milliseconds >= clickedAt + 500
    }
}
