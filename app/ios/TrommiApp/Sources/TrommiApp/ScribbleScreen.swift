import SwiftUI
import TrommiClient

struct ScribbleScreen: View {
  @EnvironmentObject var shell: AppShell
  @State private var points: [CGPoint] = []
  @State private var color = "ink"
  @State private var width = 3
  @State private var erasing = false
  var body: some View {
    VStack(spacing: 0) {
      HStack {
        Picker("Ink", selection: $color) { Text("Ink").tag("ink"); Text("Green").tag("green"); Text("Blue").tag("blue"); Text("Red").tag("red") }.pickerStyle(.menu)
        Stepper("Width", value: $width, in: 1...24)
        Toggle("Erase", isOn: $erasing)
      }.padding()
      GeometryReader { geometry in
        ZStack(alignment: .topLeading) {
          Ink.surface2
          Canvas { context, size in
            for shape in shell.view.board?.shapes ?? [] {
              if shape.tool == "pen" || shape.tool == "marker" {
                var path = Path()
                for n in stride(from: 0, to: shape.pts.count - shape.pts.count % 2, by: 2) {
                  let p = CGPoint(x: shape.pts[n], y: shape.pts[n+1]); if n == 0 { path.move(to: p) } else { path.addLine(to: p) }
                }
                context.stroke(path, with: .color(ink(shape.color ?? "ink")), style: StrokeStyle(lineWidth: shape.width ?? 3, lineCap: .round, lineJoin: .round))
              } else if let text = shape.text, shape.pts.count >= 2 { context.draw(Text(text).font(Face.text(shape.size ?? 18)), at: CGPoint(x: shape.pts[0], y: shape.pts[1]), anchor: .topLeading) }
            }
            var path = Path(); for (n, point) in points.enumerated() { if n == 0 { path.move(to: point) } else { path.addLine(to: point) } }
            context.stroke(path, with: .color(ink(color)), style: StrokeStyle(lineWidth: Double(width), lineCap: .round, lineJoin: .round))
          }
          ForEach((shell.view.board?.shapes ?? []).filter { $0.tool == "image" }, id: \.id) { shape in
            if let id = shape.attachment, shape.pts.count >= 4 { NativePicture(id: id).frame(width: abs(shape.pts[2] - shape.pts[0]), height: abs(shape.pts[3] - shape.pts[1])).offset(x: shape.pts[0], y: shape.pts[1]) }
          }
        }.contentShape(Rectangle()).gesture(DragGesture(minimumDistance: 0)
          .onChanged { value in points.append(value.location) }
          .onEnded { _ in
            guard let board = shell.view.board else { points = []; return }
            if erasing {
              // Hit testing is drawing: send the shape the pointer touched; the core applies the erase.
              let point = points.last ?? .zero
              let ids = board.shapes.filter { shape in stride(from: 0, to: shape.pts.count - shape.pts.count % 2, by: 2).contains { hypot(shape.pts[$0] - point.x, shape.pts[$0+1] - point.y) < 18 } }.map(\.id)
              shell.act(["do": "board_item", "board_id": board.boardId, "local_id": UUID().uuidString, "content": ["content_type": "erase", "stroke_ids": ids]])
            } else {
              shell.act(["do": "draw_stroke", "board_id": board.boardId, "local_id": UUID().uuidString, "tool": "pen", "color": color, "width": width, "points": points.enumerated().map { [Double($0.element.x), Double($0.element.y), Double($0.offset * 5), 1.0] }])
            }
            points = []
          })
      }
    }.navigationTitle("Scribble Board")
  }
  private func ink(_ name: String) -> Color { switch name { case "green": Ink.accent; case "blue": .blue; case "red": Ink.deny; default: Ink.fg } }
}
