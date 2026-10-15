// swift-tools-version:6.0
// Native I/O for AppCore, plus the isolated share and notification extensions.
import PackageDescription

let package = Package(
  name: "TrommiClient",
  platforms: [.iOS("27.0"), .macOS(.v15)],
  products: [
    .library(name: "TrommiClient", targets: ["TrommiClient"]),
    .library(name: "NotifyCoreLive", targets: ["NotifyCoreLive"]),
    .library(name: "ShareInbox", targets: ["ShareInbox"]),
    .library(name: "PushNotify", targets: ["PushNotify"]),
  ],
  dependencies: [
    .package(url: "https://github.com/apple/swift-crypto.git", from: "4.5.2"),
    .package(path: "../../../core/bindings/swift/TrommiCoreRust"),
  ],
  targets: [
    .target(name: "TrommiClient", dependencies: [.product(name: "Crypto", package: "swift-crypto"), .product(name: "TrommiCoreRust", package: "TrommiCoreRust")], path: "Sources/TrommiShell"),
    .target(name: "NotifyCoreLive", dependencies: ["PushNotify", .product(name: "TrommiCoreRust", package: "TrommiCoreRust")]),
    .target(name: "ShareInbox", dependencies: [.product(name: "Crypto", package: "swift-crypto")]),
    .target(name: "PushNotify", dependencies: []),
  ],
  swiftLanguageModes: [.v5]
)
