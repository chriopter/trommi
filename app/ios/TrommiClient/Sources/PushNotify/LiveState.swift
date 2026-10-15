// Native typography only. The app core chooses and orders the waiting cards.
import Foundation
public enum LiveState {
  public static func clip(_ s: String, _ max: Int) -> String {
    let flat = s.split(whereSeparator: { $0.isWhitespace || $0.isNewline }).joined(separator: " ")
    guard flat.count > max else { return flat }
    let cut = flat.prefix(max - 1)
    // at a word's end when one is near
    if let sp = cut.lastIndex(of: " "), cut.distance(from: cut.startIndex, to: sp) >= max * 2 / 3 {
      return String(cut[..<sp]) + "…"
    }
    return String(cut) + "…"
  }

}
