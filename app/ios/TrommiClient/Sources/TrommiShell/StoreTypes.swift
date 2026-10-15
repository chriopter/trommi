import Foundation
@_exported import TrommiCoreRust
public typealias Bytes = [UInt8]
public struct TrommiError: Error, CustomStringConvertible, Equatable {
  public let code: String
  public let message: String
  public init(_ code: String, _ message: String = "") { self.code = code; self.message = message }
  public var description: String { message.isEmpty ? code : "\(code): \(message)" }
}
@inline(__always) func fail(_ code: String, _ message: String = "") -> TrommiError { TrommiError(code, message) }

public let ZERO32 = Bytes(repeating: 0, count: 32)
public let ZERO16 = Bytes(repeating: 0, count: 16)

/** The kinds of a stored item (spec/v1.md section 9). 8 to 255 are reserved: read, chained, never applied. */
public struct StoreEntry: Equatable {
  public var key: Bytes
  public var value: Bytes
  public init(key: Bytes, value: Bytes) { self.key = key; self.value = value }
}
/** The changes of one operation: deletions first, then the puts; of two puts of one key the later counts. */
public struct StoreBatch: Equatable {
  public var put: [StoreEntry]
  public var delete: [Bytes]
  public init(put: [StoreEntry] = [], delete: [Bytes] = []) { self.put = put; self.delete = delete }
}
public struct StoreLoaded: Equatable {
  public var revision: UInt64
  public var entries: [StoreEntry]
  public init(revision: UInt64, entries: [StoreEntry]) { self.revision = revision; self.entries = entries }
}
public enum StoreError: Error, Equatable {
  /** The stored revision is not the one named: another owner wrote to this state. Nothing was written. */
  case conflict
  /** The platform's store failed; the text holds no stored value. Nothing was written. */
  case failed(String)
}
/**
 * A device's store, the Swift side of the core's `Storage`. `apply` writes all of the batch or none of it, makes the
 * revision `expectedRevision + 1`, and returns only once that survives a crash; it refuses with `.conflict` when
 * the stored revision differs. One owner at a time: DeviceStore.swift says how that is held on iOS.
 */
public protocol CoreStorage: AnyObject {
  func load() throws -> StoreLoaded
  func apply(expectedRevision: UInt64, batch: StoreBatch) throws
}
