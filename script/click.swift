import CoreGraphics
import Foundation
let args = CommandLine.arguments
let dx = Double(args[1])!, dy = Double(args[2])!
let list = CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as! [[String: Any]]
var origin = CGPoint.zero
for w in list {
    if let owner = w[kCGWindowOwnerName as String] as? String, owner.lowercased().contains("tuclaw"),
       let layer = w[kCGWindowLayer as String] as? Int, layer == 0,
       let b = w[kCGWindowBounds as String] as? [String: Double] {
        origin = CGPoint(x: b["X"]!, y: b["Y"]!)
        break
    }
}
let p = CGPoint(x: origin.x + dx, y: origin.y + dy)
let src = CGEventSource(stateID: .hidSystemState)
for type in [CGEventType.mouseMoved, .leftMouseDown, .leftMouseUp] {
    let e = CGEvent(mouseEventSource: src, mouseType: type, mouseCursorPosition: p, mouseButton: .left)!
    e.post(tap: .cghidEventTap)
    usleep(60000)
}
print("clicked \(p)")
