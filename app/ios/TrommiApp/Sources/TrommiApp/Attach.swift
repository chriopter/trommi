// Attach.swift: the one "Attach" of a message, a card's answer and the note, as the app had it before the shared app
// core: a menu with Photo Library, Choose File and (where there is a camera) Take Photo. The picked bytes go to the
// core as `attach`; the ids it hands back are added to `ids`.
import PhotosUI
import SwiftUI
import TrommiClient
import UniformTypeIdentifiers
#if canImport(UIKit)
import UIKit
#endif

struct AttachMenu<Label: View>: View {
  @EnvironmentObject var shell: AppShell
  @Binding var ids: [String]
  @ViewBuilder let label: () -> Label
  @State private var photos = false
  @State private var picked: [PhotosPickerItem] = []
  @State private var files = false
  @State private var camera = false
  var body: some View {
    Menu {
      Button { photos = true } label: { SwiftUI.Label("Photo Library", systemImage: "photo.on.rectangle") }
        .accessibilityIdentifier("attach-photos")
      Button { files = true } label: { SwiftUI.Label("Choose File", systemImage: "doc") }
        .accessibilityIdentifier("attach-file")
      #if canImport(UIKit)
      if UIImagePickerController.isSourceTypeAvailable(.camera) {
        Button { camera = true } label: { SwiftUI.Label("Take Photo", systemImage: "camera") }.accessibilityIdentifier("attach-camera")
      }
      #endif
    } label: { label() }
    .accessibilityLabel("Attach").accessibilityIdentifier("attach")
    .photosPicker(isPresented: $photos, selection: $picked, matching: .any(of: [.images, .videos]), preferredItemEncoding: .compatible)
    .onChange(of: picked) { _, items in
      guard !items.isEmpty else { return }
      picked = []
      Task { @MainActor in for item in items { await take(item) } }
    }
    .fileImporter(isPresented: $files, allowedContentTypes: [.item], allowsMultipleSelection: true) { result in
      if case .success(let urls) = result { for url in urls { if let id = shell.attach(url) { ids.append(id) } } }
    }
    #if canImport(UIKit)
    .sheet(isPresented: $camera) {
      CameraPicker { data in ids.append(shell.attach(data, name: "photo-\(ids.count + 1).jpg", mediaType: "image/jpeg")) }.ignoresSafeArea()
    }
    #endif
  }
  /** One picked photo or video: its bytes as the picker hands them (JPEG for a photo, `.compatible`). */
  @MainActor private func take(_ item: PhotosPickerItem) async {
    guard let data = try? await item.loadTransferable(type: Data.self) else { return }
    let type = item.supportedContentTypes.first { $0.conforms(to: .image) || $0.conforms(to: .movie) } ?? item.supportedContentTypes.first
    let movie = type?.conforms(to: .movie) == true
    let ext = type?.preferredFilenameExtension ?? (movie ? "mov" : "jpg")
    let mime = type?.preferredMIMEType ?? (movie ? "video/quicktime" : "image/jpeg")
    ids.append(shell.attach(data, name: "\(movie ? "video" : "photo")-\(ids.count + 1).\(ext)", mediaType: mime))
  }
}

#if canImport(UIKit)
/** The camera, for a photo to attach. */
struct CameraPicker: UIViewControllerRepresentable {
  let done: (Data) -> Void
  func makeCoordinator() -> C { C(done: done) }
  func makeUIViewController(context: Context) -> UIImagePickerController {
    let p = UIImagePickerController()
    p.sourceType = .camera
    p.delegate = context.coordinator
    return p
  }
  func updateUIViewController(_ c: UIImagePickerController, context: Context) {}
  final class C: NSObject, UIImagePickerControllerDelegate, UINavigationControllerDelegate {
    let done: (Data) -> Void
    init(done: @escaping (Data) -> Void) { self.done = done }
    func imagePickerController(_ p: UIImagePickerController, didFinishPickingMediaWithInfo info: [UIImagePickerController.InfoKey: Any]) {
      p.dismiss(animated: true)
      if let img = info[.originalImage] as? UIImage, let d = img.jpegData(compressionQuality: 0.9) { done(d) }
    }
    func imagePickerControllerDidCancel(_ p: UIImagePickerController) { p.dismiss(animated: true) }
  }
}
#endif
