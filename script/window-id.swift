import CoreGraphics
let list = CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as! [[String: Any]]
for w in list {
    if let owner = w[kCGWindowOwnerName as String] as? String, owner.lowercased().contains("tuclaw"),
       let layer = w[kCGWindowLayer as String] as? Int, layer == 0,
       let id = w[kCGWindowNumber as String] as? Int {
        print(id)
        break
    }
}
