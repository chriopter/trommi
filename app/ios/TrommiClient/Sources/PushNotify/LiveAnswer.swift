// LiveAnswer: the two answer buttons of the Live Activity. The widget draws them as Button(intent:); iOS runs the
// intent in the app's process (a LiveActivityIntent), where the app's handler sends its answer to AppCore
// as a tap on the card would (NativeLive.swift in the app). The widget itself answers nothing: it holds no key.
// Answering asks for the phone to be unlocked first.
import Foundation

public enum LiveAnswer {
  /** Set by the app at launch: answer card `id` with option `key`. */
  @MainActor public static var handler: ((_ card: String, _ key: String) async -> Void)?
}

#if canImport(ActivityKit) && canImport(AppIntents) && os(iOS)
import AppIntents

public struct AnswerCardIntent: LiveActivityIntent {
  public static let title: LocalizedStringResource = "Answer a question"
  public static let isDiscoverable = false
  /** An answer is his decision: not from a locked phone. */
  public static var authenticationPolicy: IntentAuthenticationPolicy { .requiresAuthentication }
  @Parameter(title: "Card") public var card: String
  @Parameter(title: "Answer") public var key: String
  public init() {}
  public init(card: String, key: String) { self.card = card; self.key = key }
  @MainActor
  public func perform() async throws -> some IntentResult {
    // (a launch for this intent: the app sets the handler as it starts)
    for _ in 0..<30 where LiveAnswer.handler == nil { try? await Task.sleep(nanoseconds: 100_000_000) }
    _ = await LiveAnswer.handler?(card, key)
    return .result()
  }
}
#endif
