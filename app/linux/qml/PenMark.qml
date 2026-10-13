import QtQuick
import "pen.mjs" as Pen

// One drawing of the web app's pen (pen.mjs, made from app/web/public/ui.mjs): its paths in their box, turned as the
// pen turned them, in one ink. `fills` gives a path's fill by its class (the crown's wash, an edge's ground).
Canvas {
    id: c
    /** The svg text the pen wrote: Pen.sketchSvg(name), Pen.doodleSvg(mark), Pen.crownSvg(), … */
    property string svg: ""
    property color ink: "black"
    /** In the box's units, as the web's CSS gives it for this drawing (a path's own stroke-width wins). */
    property real lineWidth: 2
    property var fills: ({})
    property var strokes: ({})
    /** Draw it as the web does for this kind: turned by the pen's own rotation. */
    property bool turned: true
    readonly property var parts: svg ? Pen.parts(svg) : ({ box: [0, 0, 24, 24], turn: 0, paths: [] })

    rotation: turned ? parts.turn : 0
    antialiasing: true
    renderStrategy: Canvas.Cooperative
    onSvgChanged: requestPaint()
    onInkChanged: requestPaint()
    onLineWidthChanged: requestPaint()
    onFillsChanged: requestPaint()
    onWidthChanged: requestPaint()
    onHeightChanged: requestPaint()

    onPaint: {
        const ctx = getContext("2d")
        ctx.reset()
        const box = parts.box
        if (!box[2] || !box[3]) return
        ctx.scale(width / box[2], height / box[3])
        ctx.translate(-box[0], -box[1])
        ctx.lineCap = "round"
        ctx.lineJoin = "round"
        for (const p of parts.paths) {
            ctx.beginPath()
            ctx.path = p.d
            const fill = fills[p.cls]
            if (fill) { ctx.fillStyle = fill; ctx.fill() }
            const stroke = strokes[p.cls] !== undefined ? strokes[p.cls] : ink
            if (stroke === "none") continue
            ctx.lineWidth = p.width > 0 ? p.width : lineWidth
            ctx.strokeStyle = stroke
            ctx.stroke()
        }
    }
}
