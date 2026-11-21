import CoreGraphics
import Foundation
// --top X Y: owner of the frontmost normal window under point (front-to-back order of on-screen windows)
if CommandLine.arguments.count == 4 && CommandLine.arguments[1] == "--top", let x = Double(CommandLine.arguments[2]), let y = Double(CommandLine.arguments[3]) {
  let list = CGWindowListCopyWindowInfo(.optionOnScreenOnly, kCGNullWindowID) as! [[String: Any]]
  for w in list where (w[kCGWindowLayer as String] as? Int ?? 99) < 20 {
    let b = w[kCGWindowBounds as String] as? [String: Double] ?? [:]
    if x >= b["X", default: 0] && x <= b["X", default: 0] + b["Width", default: 0] && y >= b["Y", default: 0] && y <= b["Y", default: 0] + b["Height", default: 0] {
      print(w[kCGWindowOwnerName as String] as? String ?? "?"); exit(0)
    }
  }
  print("none"); exit(0)
}
let opts = CGWindowListOption(arrayLiteral: .optionAll)
let list = CGWindowListCopyWindowInfo(opts, kCGNullWindowID) as! [[String: Any]]
for w in list {
  let owner = w[kCGWindowOwnerName as String] as? String ?? ""
  let filter = CommandLine.arguments.count > 1 ? CommandLine.arguments[1] : ""
  if !filter.isEmpty && !owner.localizedCaseInsensitiveContains(filter) { continue }
  let b = w[kCGWindowBounds as String] as? [String: Any] ?? [:]
  let layer = w[kCGWindowLayer as String] as? Int ?? 0
  let name = w[kCGWindowName as String] as? String ?? ""
  let pid = w[kCGWindowOwnerPID as String] as? Int ?? 0
  print("\(w[kCGWindowNumber as String]!)\t\(pid)\t\(owner)\t\(name)\tlayer=\(layer)\t\(b["X"]!),\(b["Y"]!) \(b["Width"]!)x\(b["Height"]!)")
}
