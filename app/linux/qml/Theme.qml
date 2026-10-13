import QtQuick

// The web board's tokens (app/web/public/app.css :root and [data-theme="dark"]), and its four tones of a hue
// (sidebar.css: oklch(L C hue + turn), wash, edge, mid, pen). As on the web, light is the default and dark only by
// the person's choice, never from the system.
QtObject {
    id: t
    property bool dark: false

    readonly property color bg: dark ? "#0e1311" : "#f5f6f2"
    readonly property color surface: dark ? "#171d1a" : "#ffffff"
    readonly property color surface2: dark ? "#1c2420" : "#fafbf8"
    readonly property color sunken: dark ? "#111715" : "#eceee8"
    readonly property color fg: dark ? "#e9eeea" : "#141c18"
    readonly property color muted: dark ? "#9aa8a0" : "#5c6862"
    readonly property color faint: dark ? "#6c7a73" : "#8a958f"
    readonly property color line: dark ? "#252f2a" : "#e1e5df"
    readonly property color lineStrong: dark ? "#35423b" : "#c9d0c8"
    readonly property color accent: dark ? "#6fd0b5" : "#1b6a57"
    readonly property color accentHover: dark ? "#8adcc5" : "#155646"
    readonly property color accentSoft: dark ? "#17332b" : "#dcefe8"
    readonly property color accentFg: dark ? "#08130f" : "#ffffff"
    readonly property color urgLow: dark ? "#8b9891" : "#6b7771"
    readonly property color urgLowSoft: dark ? "#1c2420" : "#eceee8"
    readonly property color urgNormal: dark ? "#6fd0b5" : "#1b6a57"
    readonly property color urgNormalSoft: dark ? "#17332b" : "#dcefe8"
    readonly property color urgHigh: dark ? "#f2a56c" : "#b4551b"
    readonly property color urgHighSoft: dark ? "#3a2415" : "#fbe9dc"
    readonly property color urgCritical: dark ? "#ff8a80" : "#b3261e"
    readonly property color urgCriticalSoft: dark ? "#41191a" : "#fbe0de"
    readonly property color deny: dark ? "#f08a83" : "#a8322d"
    readonly property color goldPen: dark ? "#dcb84e" : "#a8820f"
    readonly property color crownWash: "#f2c94c"
    readonly property color online: dark ? "#6cd598" : "#1f8a4c"
    // what a card's words are written in: color-mix(muted 75%, fg)
    readonly property color cardInk: mix(muted, fg, 0.75)

    // the faces (app/web/public/fonts): IBM Plex Sans for text, Bricolage Grotesque for display, Plex Mono
    property string sans: "IBM Plex Sans"
    property string display: "Bricolage Grotesque"
    property string mono: "IBM Plex Mono"

    /** a mixed with b: `amount` of a (CSS color-mix in srgb). */
    function mix(a, b, amount) {
        return Qt.rgba(a.r * amount + b.r * (1 - amount), a.g * amount + b.g * (1 - amount), a.b * amount + b.b * (1 - amount), 1)
    }
    function withAlpha(c, a) { return Qt.rgba(c.r, c.g, c.b, a) }
    /** CSS grayscale(1): the colour's luminance as a grey. */
    function grey(c) { const y = 0.2126 * c.r + 0.7152 * c.g + 0.0722 * c.b; return Qt.rgba(y, y, y, 1) }

    /** oklch to sRGB (CSS Color 4), clipped. */
    function oklch(L, C, h) {
        const a = C * Math.cos(h * Math.PI / 180), b = C * Math.sin(h * Math.PI / 180)
        const l = Math.pow(L + 0.3963377774 * a + 0.2158037573 * b, 3)
        const m = Math.pow(L - 0.1055613458 * a - 0.0638541728 * b, 3)
        const s = Math.pow(L - 0.0894841775 * a - 1.2914855480 * b, 3)
        const lin = [4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
                     -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
                     -0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s]
        const g = x => { x = Math.max(0, Math.min(1, x)); return x <= 0.0031308 ? 12.92 * x : 1.055 * Math.pow(x, 1 / 2.4) - 0.055 }
        return Qt.rgba(g(lin[0]), g(lin[1]), g(lin[2]), 1)
    }
    /** One of a hue's four tones: "wash", "edge", "mid", "pen" (162 is the bell's green). */
    function tone(hue, weight) {
        const w = dark ? { wash: [.28, .03], edge: [.4, .05], mid: [.62, .075], pen: [.8, .095] }
                       : { wash: [.95, .022], edge: [.88, .045], mid: [.64, .075], pen: [.47, .085] }
        const lc = w[weight] || w.pen
        return oklch(lc[0], lc[1], (hue === undefined || hue === null ? 162 : hue) + 10)
    }
}
