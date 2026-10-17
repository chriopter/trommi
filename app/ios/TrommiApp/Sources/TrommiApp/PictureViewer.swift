// PictureViewer: a card's (or a message's) pictures large. In the card each picture stands at the page's full width
// in its own shape; a tap opens them over the whole screen at that picture: swipe sideways through all of them, pinch
// or double-tap to zoom and pan, swipe down or the X to close. Captions come from the core (`PictureView.caption`).
import SwiftUI
#if canImport(UIKit)
import UIKit
#endif

/** One picture of a viewer: its file and the line under it. */
struct Shot: Hashable {
  let fileId: String
  let caption: String
}

/** What the viewer shows: the pictures and the one it opens at. */
struct PictureShow: Identifiable {
  let id = UUID()
  let shots: [Shot]
  let start: Int
}

/** A picture in a card's page: the page's full width, its own shape (very tall ones show their top), a tap opens it. */
struct CardPicture: View {
  @EnvironmentObject var shell: AppShell
  let fileId: String
  /** Its width / height when known before the bytes are (`PictureView.width`, `.height`). */
  let ratio: CGFloat?
  let zoom: Namespace.ID
  let open: () -> Void
  var body: some View {
    let image = loaded
    let r = ratio ?? image.map { $0.size.width / max($0.size.height, 1) } ?? 16 / 10
    Button(action: open) {
      Rectangle().fill(Ink.sunken)
        .aspectRatio(max(r, 0.4), contentMode: .fit)
        .frame(maxWidth: .infinity)
        .overlay(alignment: .top) {
          if let image { Image(uiImage: image).resizable().interpolation(.high).scaledToFill() } else { ProgressView().frame(maxHeight: .infinity) }
        }
        .clipShape(RoundedRectangle(cornerRadius: 14, style: .continuous))
        .overlay(RoundedRectangle(cornerRadius: 14, style: .continuous).strokeBorder(Ink.line))
        .contentShape(RoundedRectangle(cornerRadius: 14, style: .continuous))
    }
    .buttonStyle(.plain)
    .contextMenu { SavePictureButton(id: fileId) }
    .zoomSource(id: fileId, in: zoom)
    .accessibilityIdentifier("card-picture")
    .accessibilityAddTraits(.isImage)
    .task(id: fileId) { if shell.files[fileId] == nil { shell.act(["do": "open_file", "attachment_id": fileId, "save": false]) } }
  }
  private var loaded: UIImage? { shell.files[fileId].flatMap { UIImage(data: $0) } }
}

/** Save (the long press on a picture): the file to the phone, as a tap on it did before the viewer. */
struct SavePictureButton: View {
  @EnvironmentObject var shell: AppShell
  let id: String
  var body: some View {
    Button { shell.act(["do": "open_file", "attachment_id": id, "save": true]) } label: { Label("Save", systemImage: "square.and.arrow.down") }
  }
}

extension View {
  /** Where the viewer grows from and shrinks back to (the iPhone's zoom transition; a Mac sheet has none). */
  @ViewBuilder func zoomSource(id: String, in ns: Namespace.ID) -> some View {
    #if os(iOS)
    self.matchedTransitionSource(id: id, in: ns)
    #else
    self
    #endif
  }
  /** Opens the pictures over the whole screen, growing from the one tapped. */
  @ViewBuilder func pictureViewer(_ show: Binding<PictureShow?>, zoom: Namespace.ID, on: Bool = true) -> some View {
    if on { fullScreenCover(item: show) { s in PictureViewer(show: s, zoom: zoom) } } else { self }
  }
}

/** The pictures over the whole screen. */
struct PictureViewer: View {
  @EnvironmentObject var shell: AppShell
  @Environment(\.dismiss) private var dismiss
  let show: PictureShow
  let zoom: Namespace.ID
  @State private var at: Int
  @State private var drop: CGFloat = 0
  @State private var chrome = true
  init(show: PictureShow, zoom: Namespace.ID) {
    self.show = show
    self.zoom = zoom
    _at = State(initialValue: show.start)
  }
  var body: some View {
    let shot = show.shots[min(max(at, 0), show.shots.count - 1)]
    ZStack {
      Color.black.opacity(1 - min(drop / 500, 0.7)).ignoresSafeArea()
      pages.offset(y: drop).ignoresSafeArea()
      if chrome {
        VStack(spacing: 0) {
          HStack {
            Button { dismiss() } label: { Image(systemName: "xmark").font(.system(size: 17, weight: .semibold)).foregroundStyle(.white).pillCircle() }
              .buttonStyle(.plain).accessibilityLabel("Close").accessibilityIdentifier("picture-viewer-close")
            Spacer()
          }
          .padding(.horizontal, 16).padding(.top, 8)
          Spacer()
          VStack(spacing: 8) {
            if !shot.caption.isEmpty {
              Text(shot.caption).font(Face.text(15, .medium)).foregroundStyle(.white).multilineTextAlignment(.center).lineLimit(3)
                .accessibilityIdentifier("picture-viewer-caption")
            }
            if show.shots.count > 1 { dots }
          }
          .padding(.horizontal, 18).padding(.vertical, 12)
          .glass(RoundedRectangle(cornerRadius: 22, style: .continuous), tint: .black.opacity(0.25))
          .padding(.horizontal, 16).padding(.bottom, 12)
          .opacity(shot.caption.isEmpty && show.shots.count < 2 ? 0 : 1)
        }
        .opacity(1 - min(drop / 160, 1))
        .transition(.opacity)
      }
    }
    .environment(\.colorScheme, .dark)
    .hidesStatusBar(!chrome)
    // (pulled down, the page under it shows through)
    .presentationBackground(.clear)
    .zoomTarget(id: shot.fileId, in: zoom)
  }

  /** Where it stands: a dot per picture, up to twelve; more as "3 of 20". */
  @ViewBuilder private var dots: some View {
    if show.shots.count <= 12 {
      HStack(spacing: 7) {
        ForEach(show.shots.indices, id: \.self) { i in
          Circle().fill(.white.opacity(i == at ? 1 : 0.35)).frame(width: 7, height: 7)
        }
      }
      .accessibilityElement().accessibilityLabel("Picture \(at + 1) of \(show.shots.count)").accessibilityIdentifier("picture-viewer-page")
    } else {
      Text("\(at + 1) of \(show.shots.count)").font(Face.text(13, .medium)).foregroundStyle(.white.opacity(0.8)).monospacedDigit()
        .accessibilityIdentifier("picture-viewer-page")
    }
  }

  @ViewBuilder private var pages: some View {
    #if os(iOS)
    TabView(selection: $at) {
      ForEach(show.shots.indices, id: \.self) { i in
        ZoomPage(fileId: show.shots[i].fileId,
                 tap: { withAnimation(.easeOut(duration: 0.2)) { chrome.toggle() } },
                 drag: { drop = max($0, 0) },
                 release: { dy, speed in
                   if dy > 110 || speed > 900 { dismiss() } else { withAnimation(.spring(duration: 0.3)) { drop = 0 } }
                 })
          .tag(i)
      }
    }
    .tabViewStyle(.page(indexDisplayMode: .never))
    #else
    // (a Mac: one picture at a time, arrows to go on)
    HStack {
      Button { at = max(at - 1, 0) } label: { Image(systemName: "chevron.left").font(.title2) }.disabled(at == 0)
      if let data = shell.files[show.shots[at].fileId], let image = UIImage(data: data) { Image(uiImage: image).resizable().scaledToFit().frame(maxWidth: .infinity, maxHeight: .infinity) } else { ProgressView().frame(maxWidth: .infinity, maxHeight: .infinity) }
      Button { at = min(at + 1, show.shots.count - 1) } label: { Image(systemName: "chevron.right").font(.title2) }.disabled(at >= show.shots.count - 1)
    }
    .buttonStyle(.plain).foregroundStyle(.white).padding(.horizontal, 12).padding(.vertical, 70)
    #endif
  }
}

extension View {
  @ViewBuilder fileprivate func hidesStatusBar(_ hidden: Bool) -> some View {
    #if os(iOS)
    self.statusBarHidden(hidden)
    #else
    self
    #endif
  }
  @ViewBuilder fileprivate func zoomTarget(id: String, in ns: Namespace.ID) -> some View {
    #if os(iOS)
    self.navigationTransition(.zoom(sourceID: id, in: ns)).interactiveDismissDisabled()
    #else
    self
    #endif
  }
}

#if os(iOS)
/** One page of the viewer: the picture, zoomable. */
private struct ZoomPage: View {
  @EnvironmentObject var shell: AppShell
  let fileId: String
  let tap: () -> Void
  let drag: (CGFloat) -> Void
  let release: (CGFloat, CGFloat) -> Void
  var body: some View {
    Group {
      if let data = shell.files[fileId], let image = UIImage(data: data) {
        ZoomableImage(image: image, tap: tap, drag: drag, release: release)
      } else {
        ProgressView().tint(.white).frame(maxWidth: .infinity, maxHeight: .infinity)
      }
    }
    .task(id: fileId) { if shell.files[fileId] == nil { shell.act(["do": "open_file", "attachment_id": fileId, "save": false]) } }
  }
}

/** UIKit's scroll view for the zoom: pinch, double-tap to a point and back, pan while zoomed, and a downward pull
 *  (only while not zoomed) that the viewer turns into closing. */
private struct ZoomableImage: UIViewRepresentable {
  let image: UIImage
  let tap: () -> Void
  let drag: (CGFloat) -> Void
  let release: (CGFloat, CGFloat) -> Void

  func makeUIView(context: Context) -> ZoomView {
    let v = ZoomView()
    v.imageView.image = image
    v.onTap = tap; v.onDrag = drag; v.onRelease = release
    return v
  }
  func updateUIView(_ v: ZoomView, context: Context) {
    if v.imageView.image !== image { v.imageView.image = image; v.zoomScale = 1; v.setNeedsLayout() }
    v.onTap = tap; v.onDrag = drag; v.onRelease = release
  }
}

final class ZoomView: UIScrollView, UIScrollViewDelegate {
  let imageView = UIImageView()
  var onTap: () -> Void = {}
  var onDrag: (CGFloat) -> Void = { _ in }
  var onRelease: (CGFloat, CGFloat) -> Void = { _, _ in }
  private let pull = UIPanGestureRecognizer()
  private lazy var gate = PullGate(view: self)
  private var laidFor: CGSize = .zero

  override init(frame: CGRect) {
    super.init(frame: frame)
    delegate = self
    minimumZoomScale = 1
    maximumZoomScale = 6
    bouncesZoom = true
    showsVerticalScrollIndicator = false
    showsHorizontalScrollIndicator = false
    contentInsetAdjustmentBehavior = .never
    decelerationRate = .fast
    backgroundColor = .clear
    imageView.contentMode = .scaleAspectFit
    imageView.isUserInteractionEnabled = true
    imageView.accessibilityIdentifier = "picture-viewer-image"
    addSubview(imageView)
    let double = UITapGestureRecognizer(target: self, action: #selector(doubleTap(_:)))
    double.numberOfTapsRequired = 2
    addGestureRecognizer(double)
    let single = UITapGestureRecognizer(target: self, action: #selector(singleTap))
    single.require(toFail: double)
    addGestureRecognizer(single)
    pull.addTarget(self, action: #selector(pulled(_:)))
    pull.delegate = gate
    addGestureRecognizer(pull)
  }
  required init?(coder: NSCoder) { fatalError("init(coder:) is not used") }

  /** The picture fitted into the page at zoom 1. */
  private var fitted: CGSize {
    guard let s = imageView.image?.size, s.width > 0, s.height > 0, bounds.width > 0, bounds.height > 0 else { return bounds.size }
    let k = min(bounds.width / s.width, bounds.height / s.height)
    return CGSize(width: s.width * k, height: s.height * k)
  }

  override func layoutSubviews() {
    super.layoutSubviews()
    if laidFor != bounds.size {
      laidFor = bounds.size
      zoomScale = 1
      imageView.frame = CGRect(origin: .zero, size: fitted)
      contentSize = fitted
      // (at most so far that a picture's pixels stand at three times the screen's)
      if let s = imageView.image?.size, fitted.width > 0 { maximumZoomScale = max(3, s.width * (imageView.image?.scale ?? 1) / fitted.width * 3 / max(traitCollection.displayScale, 1)) }
    }
    center()
  }

  /** The picture stays in the middle while it is smaller than the page. */
  private func center() {
    let dx = max((bounds.width - contentSize.width) / 2, 0), dy = max((bounds.height - contentSize.height) / 2, 0)
    contentInset = UIEdgeInsets(top: dy, left: dx, bottom: dy, right: dx)
  }

  func viewForZooming(in scrollView: UIScrollView) -> UIView? { imageView }
  func scrollViewDidZoom(_ scrollView: UIScrollView) { center() }

  @objc private func singleTap() { onTap() }

  @objc private func doubleTap(_ g: UITapGestureRecognizer) {
    if zoomScale > minimumZoomScale + 0.01 {
      setZoomScale(minimumZoomScale, animated: true)
    } else {
      let p = g.location(in: imageView), k = min(maximumZoomScale, 2.5)
      let size = CGSize(width: bounds.width / k, height: bounds.height / k)
      zoom(to: CGRect(x: p.x - size.width / 2, y: p.y - size.height / 2, width: size.width, height: size.height), animated: true)
    }
  }

  @objc private func pulled(_ g: UIPanGestureRecognizer) {
    let dy = g.translation(in: self).y
    switch g.state {
    case .changed: onDrag(dy)
    case .ended: onRelease(dy, g.velocity(in: self).y)
    case .cancelled, .failed: onRelease(0, 0)
    default: break
    }
  }

  /** Whether a pull may start now: only on a picture at its fitted size. */
  var atRest: Bool { zoomScale <= minimumZoomScale + 0.01 }
}

/** The pull starts only on a picture at its fitted size, downward and more down than sideways: sideways is the next
 *  picture, and a zoomed picture pans. */
@MainActor private final class PullGate: NSObject, UIGestureRecognizerDelegate {
  weak var view: ZoomView?
  init(view: ZoomView) { self.view = view }
  func gestureRecognizerShouldBegin(_ g: UIGestureRecognizer) -> Bool {
    guard let view, let pan = g as? UIPanGestureRecognizer else { return false }
    let v = pan.velocity(in: view)
    return view.atRest && v.y > 0 && abs(v.y) > abs(v.x) * 1.3
  }
}
#endif
