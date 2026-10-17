import AVFoundation
import Foundation
import WebKit

/// What only the phone can do for sealed dictation (spec/v1.md 18; the core's platform ops record_start, record_stop,
/// record_cancel, attest): the microphone, recorded as 16 kHz mono 16-bit WAV, and the check of Tinfoil's attestation
/// bundle with Tinfoil's own verifier (the web's, `Resources/tinfoil-verifier.js`, run offline in a hidden web view,
/// which has the Web Crypto it needs). Nothing here decides: when to record, whether to send, what to send is the core's.
@MainActor final class NativeDictation: NSObject {
  static let shared = NativeDictation()
  private var recorder: AVAudioRecorder?
  private var file: URL?
  private var started = Date()
  private var asking = 0
  private var verifier: WKWebView?

  /// Whether this device can dictate at all (a microphone and the verifier).
  static var can: Bool {
    #if os(iOS)
    return Bundle.module.url(forResource: "tinfoil-verifier", withExtension: "js") != nil
    #else
    return false
    #endif
  }

  /// The microphone's level now, 0...1, for the bars while recording.
  var level: Double {
    guard let recorder, recorder.isRecording else { return 0 }
    recorder.updateMeters()
    return max(0, min(1, (Double(recorder.averagePower(forChannel: 0)) + 50) / 50))
  }

  func op(_ op: [String: Any], blobs: [Data]) async -> (answer: [String: Any], blobs: [Data]) {
    switch op["op"] as? String {
    case "record_start": return await start()
    case "record_stop": return stop()
    case "record_cancel": cancel(); return (["answer": "done"], [])
    case "attest":
      guard let i = op["bundle"] as? Int, blobs.indices.contains(i), let repo = op["repo"] as? String else { return Self.notAttested("no bundle") }
      return await attest(bundle: blobs[i], repo: repo)
    default: return (["answer": "refused"], [])
    }
  }

  private func start() async -> (answer: [String: Any], blobs: [Data]) {
    cancel()
    asking += 1
    let mine = asking
    let allowed = await AVAudioApplication.requestRecordPermission()
    guard allowed, mine == asking else { return (["answer": "refused"], []) }
    #if os(iOS)
    do {
      try AVAudioSession.sharedInstance().setCategory(.playAndRecord, mode: .spokenAudio, options: [.defaultToSpeaker, .allowBluetoothHFP])
      try AVAudioSession.sharedInstance().setActive(true)
    } catch { return (["answer": "refused"], []) }
    #endif
    let url = FileManager.default.temporaryDirectory.appendingPathComponent("dictation-\(UUID().uuidString).wav")
    let settings: [String: Any] = [AVFormatIDKey: kAudioFormatLinearPCM, AVSampleRateKey: 16000, AVNumberOfChannelsKey: 1,
                                   AVLinearPCMBitDepthKey: 16, AVLinearPCMIsFloatKey: false, AVLinearPCMIsBigEndianKey: false]
    guard let recorder = try? AVAudioRecorder(url: url, settings: settings) else { return (["answer": "refused"], []) }
    recorder.isMeteringEnabled = true
    guard recorder.record(forDuration: 300) else { try? FileManager.default.removeItem(at: url); return (["answer": "refused"], []) }
    self.recorder = recorder
    file = url
    started = Date()
    return (["answer": "recording"], [])
  }

  private func stop() -> (answer: [String: Any], blobs: [Data]) {
    guard let recorder, let file else { return (["answer": "refused"], []) }
    let ms = Int(Date().timeIntervalSince(started) * 1000)
    recorder.stop()
    self.recorder = nil
    self.file = nil
    deactivate()
    defer { try? FileManager.default.removeItem(at: file) }
    guard let wav = try? Data(contentsOf: file) else { return (["answer": "refused"], []) }
    return (["answer": "recorded", "audio": 0, "ms": ms], [wav])
  }

  private func cancel() {
    asking += 1
    recorder?.stop()
    recorder = nil
    if let file { try? FileManager.default.removeItem(at: file) }
    file = nil
    deactivate()
  }

  private func deactivate() {
    #if os(iOS)
    try? AVAudioSession.sharedInstance().setActive(false, options: .notifyOthersOnDeactivation)
    #endif
  }

  private static func notAttested(_ reason: String) -> (answer: [String: Any], blobs: [Data]) {
    (["answer": "not_attested", "reason": String(reason.prefix(200))], [])
  }

  /// Tinfoil's verifier on the bundle the hub passed on, pinned to the core's repo: the enclave's HPKE key and host.
  private func attest(bundle: Data, repo: String) async -> (answer: [String: Any], blobs: [Data]) {
    guard let parsed = try? JSONSerialization.jsonObject(with: bundle) as? [String: Any], let domain = parsed["domain"] as? String,
          let text = String(data: bundle, encoding: .utf8) else { return Self.notAttested("not a bundle") }
    do {
      let web = try await verifierView()
      let found = try await web.callAsyncJavaScript(
        "const r = await new TinfoilVerifier.Verifier({ configRepo: repo }).verifyBundle(JSON.parse(bundle)); return r.hpkePublicKey ?? ''",
        arguments: ["bundle": text, "repo": repo], in: nil, contentWorld: .defaultClient)
      guard let hex = found as? String, hex.count == 64, let key = Data(hex: hex) else { return Self.notAttested("no HPKE key") }
      return (["answer": "attested", "hpke_key": 0, "domain": domain], [key])
    } catch {
      return Self.notAttested(String(describing: error))
    }
  }

  /// A web view that holds nothing but the verifier, on a secure origin it never fetches from (Web Crypto needs one).
  private func verifierView() async throws -> WKWebView {
    if let verifier { return verifier }
    guard let url = Bundle.module.url(forResource: "tinfoil-verifier", withExtension: "js"), let script = try? String(contentsOf: url, encoding: .utf8) else {
      throw NSError(domain: "trommi.dictation", code: 1, userInfo: [NSLocalizedDescriptionKey: "the verifier is missing"])
    }
    let configuration = WKWebViewConfiguration()
    configuration.websiteDataStore = .nonPersistent()
    let web = WKWebView(frame: .zero, configuration: configuration)
    let loaded = Loaded()
    web.navigationDelegate = loaded
    web.loadHTMLString("<!doctype html><meta charset=utf-8><title>verifier</title>", baseURL: URL(string: "https://verifier.trommi.invalid/"))
    try await loaded.wait()
    _ = try await web.callAsyncJavaScript(script + "\n;return typeof TinfoilVerifier", arguments: [:], in: nil, contentWorld: .defaultClient)
    web.navigationDelegate = nil
    verifier = web
    return web
  }
}

private final class Loaded: NSObject, WKNavigationDelegate {
  private var done: CheckedContinuation<Void, Error>?
  private var finished = false
  func wait() async throws {
    if finished { return }
    try await withCheckedThrowingContinuation { done = $0 }
  }
  func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) { finished = true; done?.resume(); done = nil }
  func webView(_ webView: WKWebView, didFail navigation: WKNavigation!, withError error: Error) { done?.resume(throwing: error); done = nil }
}

private extension Data {
  init?(hex: String) {
    var out = Data()
    var i = hex.startIndex
    while i < hex.endIndex {
      let j = hex.index(i, offsetBy: 2)
      guard let b = UInt8(hex[i..<j], radix: 16) else { return nil }
      out.append(b)
      i = j
    }
    self = out
  }
}
