// The view model's fold for a Swift shell: what core/client/src/views/mod.rs `ViewModel::apply` does, in the same
// order, over the generated types (Views.swift). A shell keeps one model and folds every `Effect::Render` delta in.
// Checked by tests/bindings/views-replay.swift against the recorded scenes (core/client/tests/conformance/views).
import Foundation

extension CoreView {
  /// A delta whose `base` is not the model's `rev`: a shell bug, the delta is not folded.
  public struct OutOfOrder: Error, Equatable {
    public let expected: UInt64
    public let got: UInt64
  }
}

extension CoreView.ViewModel {
  /// Folds one delta in.
  public mutating func apply(_ delta: CoreView.ViewDelta) throws {
    let whole = delta.parts.contains { if case .whole = $0 { return true } else { return false } }
    if !whole && delta.base != rev { throw CoreView.OutOfOrder(expected: rev, got: delta.base) }
    for part in delta.parts {
      switch part {
      case .whole(let model): self = model
      case .phase(let phase, let status): self.phase = phase; self.status = status
      case .sidebar(let view): sidebar = view
      case .desk(let view): desk = view
      case .session(let id, let view): sessions[id] = view
      case .card(let id, let view): cards[id] = view
      case .off(let view): off = view
      case .notes(let view): notes = view
      case .settings(let view): settings = view
      case .account(let view): account = view
      case .file(let id, let view): files[id] = view
      case .toasts(let list): toasts = list
      case .board(let view): board = view
      }
    }
    rev = delta.base + 1
  }
}
