// MacShims: what the shared views ask of UIKit and of SwiftUI's iPhone-only modifiers, given its Mac counterpart or
// nothing, so that the views stay one source for both (app/ios/README.md "On a Mac"). Only names whose meaning
// is the same on both sides, or that mean nothing on a Mac (the title's size in a bar, the keyboard's kind), belong
// here; everything else is behind #if os(iOS) where it is used. A declaration here wins over SwiftUI's own of the
// same name because SwiftUI's is unavailable on macOS.
#if os(macOS)
import AppKit
import SwiftUI

typealias UIImage = NSImage

extension Image {
  /** `Image(uiImage:)` of the shared views. */
  init(uiImage: NSImage) { self.init(nsImage: uiImage) }
}

/** The bar's title size: a Mac window has one title. */
enum NavigationBarItem { enum TitleDisplayMode { case automatic, inline, large } }
/** The kind of keyboard: a Mac has one. */
enum UIKeyboardType { case `default`, emailAddress, numberPad, URL }
/** Capitals the iPhone's keyboard would put: a Mac's keyboard puts none on its own. */
struct TextInputAutocapitalization { static let never = Self(), words = Self(), sentences = Self(), characters = Self() }

extension View {
  func navigationBarTitleDisplayMode(_ mode: NavigationBarItem.TitleDisplayMode) -> some View { self }
  func keyboardType(_ type: UIKeyboardType) -> some View { self }
  func textInputAutocapitalization(_ a: TextInputAutocapitalization?) -> some View { self }
}

extension ToolbarItemPlacement {
  /** The iPhone's bar corners: the window's toolbar on a Mac. */
  static var topBarTrailing: ToolbarItemPlacement { .primaryAction }
  static var topBarLeading: ToolbarItemPlacement { .navigation }
}
#endif

#if os(macOS)
typealias UIColor = NSColor

/** The light or dark of the moment, as UIKit's trait collection names it (`$0.userInterfaceStyle == .dark`). */
struct MacTraits {
  enum Style { case light, dark }
  let userInterfaceStyle: Style
}

extension NSColor {
  /** UIKit's `UIColor { traits in … }`: a colour that follows the window's light or dark appearance. */
  convenience init(dynamicProvider: @escaping (MacTraits) -> NSColor) {
    self.init(name: nil) { a in
      dynamicProvider(MacTraits(userInterfaceStyle: a.bestMatch(from: [.darkAqua, .aqua]) == .darkAqua ? .dark : .light))
    }
  }
}
#endif

#if os(macOS)
/** The iPhone's navigation bar, named by the shared views to hide it or its background: a Mac window keeps its toolbar. */
enum MacNoBar { case navigationBar }

extension View {
  func toolbar(_ visibility: Visibility, for bar: MacNoBar) -> some View { self }
  func toolbarBackground(_ visibility: Visibility, for bar: MacNoBar) -> some View { self }
  /** A cover over the whole screen on the iPhone: a sheet of a good size on a Mac. */
  func fullScreenCover<Item: Identifiable, Content: View>(item: Binding<Item?>, onDismiss: (() -> Void)? = nil,
                                                          @ViewBuilder content: @escaping (Item) -> Content) -> some View {
    sheet(item: item, onDismiss: onDismiss) { content($0).frame(minWidth: 720, idealWidth: 960, minHeight: 540, idealHeight: 720) }
  }
}

/** The keyboard's notifications: a Mac's keyboard never covers the window, so they never come. */
enum UIResponder {
  static let keyboardWillShowNotification = Notification.Name("trommi.mac.never.keyboardWillShow")
  static let keyboardWillHideNotification = Notification.Name("trommi.mac.never.keyboardWillHide")
  static let keyboardWillChangeFrameNotification = Notification.Name("trommi.mac.never.keyboardWillChangeFrame")
  static let keyboardFrameEndUserInfoKey = "trommi.mac.never.keyboardFrameEnd"
}
#endif
