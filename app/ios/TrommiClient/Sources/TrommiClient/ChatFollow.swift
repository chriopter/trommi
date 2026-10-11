// ChatFollow.swift: whether a conversation follows what comes in, and the "N new" pill when it does not
// (SessionScreen). The pill counts what arrived while he was away from the end; reaching the end, or seeing the
// newest message, clears it.

/** A conversation's place: at its end (it follows what comes in), or away, with what arrived meanwhile. */
public struct ChatFollow: Equatable {
  public private(set) var atBottom = true
  public private(set) var unseen = 0
  public init() {}

  /** `n` new messages arrived; `mine`: the newest is his own. True: the view goes to the end. */
  public mutating func arrived(_ n: Int, mine: Bool) -> Bool {
    guard n > 0 else { return false }
    if atBottom || mine { unseen = 0; return true }
    unseen += n
    return false
  }
  /** His scrolling ended this far (points) above the end of the content; within `near` counts as the end. */
  public mutating func scrolled(toEnd distance: Double, near: Double = 80) {
    if distance <= near { reachedEnd() } else { atBottom = false }
  }
  /** He left the end on purpose (an earlier page, a jump up): new messages are counted from now. */
  public mutating func left() { atBottom = false }
  /** The end of the conversation is on screen (or the view was taken there): it follows again, nothing is unseen. */
  public mutating func reachedEnd() { atBottom = true; unseen = 0 }
  /** The newest message became visible: what came in is seen, whether or not the very end is. */
  public mutating func sawNewest() { unseen = 0 }
}
