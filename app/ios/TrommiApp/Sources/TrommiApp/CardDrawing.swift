// CardDrawing.swift: "Draw on the card" (the core's card menu, as on the web): the screen as it stands, a pen over it,
// and the drawing with the card under it as one picture in the card's field, sent with the next words.
import SwiftUI
#if os(iOS)
import UIKit
#endif

/** The screen when the pen was asked for (only the iPhone and iPad draw on a card). */
struct DrawShot: Identifiable {
  let id = UUID()
  #if os(iOS)
  let image: UIImage
  #endif
}

#if canImport(PencilKit) && os(iOS)
import PencilKit

struct CardDrawing: View {
  let shot: UIImage
  let cancel: String
  let done: String
  let attach: (Data) -> Void
  @Environment(\.dismiss) private var dismiss
  @State private var canvas = PKCanvasView()
  init(shot: UIImage, cancel: String, done: String, attach: @escaping (Data) -> Void) {
    self.shot = shot; self.cancel = cancel; self.done = done; self.attach = attach
  }
  /** The key window as it stands now. */
  @MainActor static func snapshot() -> DrawShot? {
    guard let w = UIApplication.shared.connectedScenes.compactMap({ ($0 as? UIWindowScene)?.keyWindow }).first else { return nil }
    let image = UIGraphicsImageRenderer(bounds: w.bounds).image { _ in _ = w.drawHierarchy(in: w.bounds, afterScreenUpdates: false) }
    return DrawShot(image: image)
  }
  var body: some View {
    ZStack {
      Image(uiImage: shot).resizable().ignoresSafeArea()
      DrawCanvas(canvas: canvas).ignoresSafeArea()
    }
    .overlay(alignment: .top) {
      HStack {
        Button(cancel) { dismiss() }.font(.body.weight(.semibold)).foregroundStyle(Ink.fg).padding(.horizontal, 18).frame(height: 44).glass(Capsule(), interactive: true)
        Spacer()
        Button(done) {
          let size = shot.size
          let png = UIGraphicsImageRenderer(size: size).pngData { _ in
            shot.draw(in: CGRect(origin: .zero, size: size))
            canvas.drawing.image(from: CGRect(origin: .zero, size: size), scale: shot.scale).draw(in: CGRect(origin: .zero, size: size))
          }
          attach(png)
          dismiss()
        }
        .font(.body.weight(.semibold)).foregroundStyle(Ink.accentFg).padding(.horizontal, 18).frame(height: 44).glass(Capsule(), interactive: true, tint: Ink.accent)
      }
      .padding(.horizontal, 16).padding(.top, 8)
    }
    .statusBarHidden()
  }
}

/** PencilKit's canvas, clear over the picture: a red pen, finger and Pencil both draw. */
private struct DrawCanvas: UIViewRepresentable {
  let canvas: PKCanvasView
  func makeUIView(context: Context) -> PKCanvasView {
    canvas.backgroundColor = .clear
    canvas.isOpaque = false
    canvas.drawingPolicy = .anyInput
    canvas.tool = PKInkingTool(.pen, color: .systemRed, width: 5)
    return canvas
  }
  func updateUIView(_ v: PKCanvasView, context: Context) {}
}
#endif
