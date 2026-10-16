import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import "pen.mjs" as Pen

// The sidebar, drawn from the app core's view model alone (ViewModel.sidebar, core/client/src/views): the team in
// view with a dropdown of the teams, then one row per session of that team in the order given, each with its
// drawing, its name, the small line under it, and its badge. Nothing here is counted, ordered or worded: the only
// state of this file's own is which mains the person unfolded.
Rectangle {
    id: root
    required property var theme
    /** ViewModel.sidebar */
    required property var sidebar
    signal theme_toggled()
    /** Another page: an `Event::Focus` screen (the Desk, a session). */
    signal go(var screen)
    /** A team chosen in the dropdown (`TeamTab.id`): the window sends it as `Focus.team`. */
    signal team_chosen(string id)
    color: theme.bg

    /** The mains the person unfolded here (their helpers show as rows of their own). */
    property var unfolded: ({})
    readonly property var all: sidebar.sessions || []
    /** The rows by id, for a helper's drawing on its main's edge. */
    readonly property var byId: { const m = {}; for (const s of all) m[s.id] = s; return m }
    readonly property var shown: all.filter(s => s.depth === 0 || !!unfolded[s.parent])

    function hueOf(s) { return Pen.hueFor({ id: s.slug || s.id, mark: s.mark }) }
    // the drawing of the small line's kind (sidebar.css)
    function capSign(kind) { return kind === "gone" ? "plug" : kind === "cut" ? "ear-off" : "ear-later" }
    // the tally: a stroke for each question up to four, the fifth across them (ui.mjs tallySvg)
    function tallySvg(n) {
        const T = ['M3.9 2.2Q2.6 7.6 2.5 13.9', 'M8.6 1.6Q8.4 8.4 7.3 13.2', 'M13.7 2.6Q12.5 7.2 12.6 14.2', 'M18.6 1.9Q18.3 8.6 17.2 13.5']
        const w = n > 4 ? 22 : n * 5 + 1
        let s = '<svg viewBox="0 0 ' + w + ' 16">'
        for (let i = 0; i < Math.min(n, 4); i++) s += '<path d="' + T[i] + '"/>'
        if (n > 4) s += '<path d="M.9 11.8Q10.5 7.6 21.1 3.5"/>'
        return s + '</svg>'
    }

    // ---- the team in view: its name, and a dropdown of the teams, each with what waits there ----
    RowLayout {
        id: head
        anchors { left: parent.left; right: parent.right; top: parent.top; leftMargin: 18; rightMargin: 24; topMargin: 15 }
        height: 40
        spacing: 12
        // the drawing opens the Desk
        PenMark {
            Layout.preferredWidth: 34; Layout.preferredHeight: 34; svg: Pen.sketchSvg("desk"); ink: theme.fg; lineWidth: 1.7
            MouseArea { anchors.fill: parent; cursorShape: Qt.PointingHandCursor; onClicked: root.go({ screen: "desk" }) }
        }
        Item {
            Layout.fillWidth: true
            Layout.fillHeight: true
            Accessible.role: Accessible.ComboBox
            Accessible.name: face.text
            RowLayout {
                anchors.fill: parent
                spacing: 8
                Label {
                    id: face
                    text: root.sidebar.place ? root.sidebar.place.en : ""
                    font.family: theme.display; font.pixelSize: 23; font.weight: Font.ExtraBold; font.letterSpacing: -0.4
                    color: theme.fg
                    elide: Text.ElideRight
                    Layout.fillWidth: true
                }
                PenMark { Layout.preferredWidth: 14; Layout.preferredHeight: 14; svg: Pen.sketchSvg("unfold"); ink: theme.muted; lineWidth: 2 }
            }
            MouseArea { anchors.fill: parent; cursorShape: Qt.PointingHandCursor; onClicked: teams.open() }
        }
    }
    Popup {
        id: teams
        x: head.x
        y: head.y + head.height + 4
        width: head.width
        padding: 6
        background: Rectangle { color: theme.surface; radius: 12; border.width: 1; border.color: theme.lineStrong }
        contentItem: Column {
            spacing: 2
            Repeater {
                model: root.sidebar.teams || []
                delegate: Rectangle {
                    id: entry
                    required property var modelData
                    width: teams.availableWidth
                    height: 36
                    radius: 8
                    color: modelData.selected || hover.hovered ? theme.sunken : "transparent"
                    HoverHandler { id: hover }
                    RowLayout {
                        anchors { fill: parent; leftMargin: 10; rightMargin: 10 }
                        spacing: 8
                        Label {
                            text: entry.modelData.name ? entry.modelData.name.en : ""
                            color: theme.fg; font.pixelSize: 15
                            font.weight: entry.modelData.selected ? Font.Bold : Font.Normal
                            elide: Text.ElideRight
                            Layout.fillWidth: true
                        }
                        Label {
                            visible: entry.modelData.waiting > 0
                            text: entry.modelData.waiting
                            color: entry.modelData.knocks ? theme.urgHigh : theme.muted
                            font.pixelSize: 12; font.weight: Font.Bold
                        }
                    }
                    MouseArea {
                        anchors.fill: parent
                        cursorShape: Qt.PointingHandCursor
                        onClicked: { teams.close(); root.team_chosen(entry.modelData.id) }
                    }
                }
            }
        }
    }

    ListView {
        id: list
        anchors { left: parent.left; right: parent.right; top: head.bottom; bottom: foot.top; leftMargin: 16; rightMargin: 9; topMargin: 22 }
        clip: true
        model: root.shown
        ScrollBar.vertical: ScrollBar { policy: ScrollBar.AsNeeded; width: 6 }
        delegate: Column {
            id: slot
            required property var modelData
            required property int index
            readonly property var s: modelData
            readonly property bool sub: s.depth > 0
            readonly property bool isFolded: !!s.folded && !root.unfolded[s.id]
            /** What the row shows: folded, the core's word for the main and its helpers together. */
            readonly property var face: isFolded ? s.folded : s
            readonly property real hue: root.hueOf(s)
            readonly property var edges: isFolded ? (s.subs || []).slice(0, 6) : []
            width: list.width - 2

            Item {
                id: card
                width: parent.width
                height: entry.height + (slot.edges.length ? 12 + slot.edges.length * 2.4 : 0) + (slot.sub ? 6 : 34)
                opacity: slot.s.hushed ? 0.6 : 1

                // the helpers' edges under a folded main: one each, in its own hue; a stopped one in the warning's
                Repeater {
                    model: slot.edges
                    delegate: PenMark {
                        required property var modelData
                        required property int index
                        readonly property var sub: root.byId[modelData] || ({ id: modelData })
                        readonly property var quirk: Pen.edgeQuirk(sub.slug || sub.id)
                        readonly property real h: root.hueOf(sub)
                        readonly property bool stopped: ((slot.s.folded || {}).stopped || []).indexOf(modelData) >= 0
                        readonly property color ground: theme.dark ? theme.mix(theme.tone(h, "edge"), theme.bg, .3) : theme.tone(h, "edge")
                        x: 7 + Number(quirk.dx)
                        y: entry.height - 10 + (slot.edges.length - index) * 2.4
                        z: -(slot.edges.length - index)
                        width: entry.width - 14; height: 16
                        rotation: Number(quirk.tilt)
                        turned: false
                        svg: quirk.svg
                        ink: stopped ? theme.urgCritical : theme.dark ? theme.mix(theme.tone(h, "pen"), theme.bg, .45) : theme.tone(h, "pen"); lineWidth: 1.3
                        fills: ({ "e-w": ground, "e-n": ground })
                        MouseArea { anchors.fill: parent; cursorShape: Qt.PointingHandCursor; onClicked: { const u = Object.assign({}, root.unfolded); u[slot.s.id] = true; root.unfolded = u } }
                    }
                }

                Rectangle {
                    id: entry
                    x: slot.sub ? 20 : 0
                    width: card.width - x
                    height: slot.sub ? 42 : 51
                    radius: slot.sub ? 12 : 14
                    color: theme.surface
                    border.width: 1
                    border.color: theme.lineStrong

                    PenMark {
                        id: mark
                        x: slot.sub ? 10 : 12; anchors.verticalCenter: parent.verticalCenter
                        width: slot.sub ? 28 : 38; height: width
                        svg: Pen.doodleSvg(slot.s.mark || slot.s.slug || slot.s.id)
                        ink: slot.face.online ? theme.tone(slot.hue, "pen") : theme.grey(theme.tone(slot.hue, "pen"))
                        opacity: slot.face.online ? 1 : 0.6
                        lineWidth: 2
                    }
                    PenMark {
                        visible: !!slot.s.crowned
                        x: mark.x + 3; y: -8
                        width: 25; height: 18
                        svg: Pen.crownSvg(); turned: false
                        ink: theme.goldPen; lineWidth: 1.7
                        fills: ({ "crown-wash": theme.crownWash })
                        strokes: ({ "crown-wash": "none" })
                    }
                    ColumnLayout {
                        anchors { left: mark.right; right: badge.left; verticalCenter: parent.verticalCenter; leftMargin: 12; rightMargin: 6 }
                        spacing: 0
                        Label {
                            text: slot.s.name ? slot.s.name.en : ""
                            color: slot.face.online ? theme.fg : theme.muted
                            font.pixelSize: slot.sub ? 14 : 17; font.weight: slot.sub ? Font.DemiBold : Font.Bold
                            elide: Text.ElideRight
                            Layout.fillWidth: true
                        }
                        RowLayout {
                            visible: !!slot.s.cap
                            spacing: 3
                            PenMark { Layout.preferredWidth: 16; Layout.preferredHeight: 16; svg: slot.s.cap ? Pen.sketchSvg(root.capSign(slot.s.cap.kind)) : ""; ink: theme.faint; lineWidth: 1.9 }
                            Label { text: slot.s.cap ? slot.s.cap.word.en : ""; color: theme.faint; font.pixelSize: 12 }
                        }
                    }
                    // the badge: the raised hand when it is stopped, else its open questions as a tally or a figure
                    Item {
                        id: badge
                        anchors { right: parent.right; rightMargin: 10; verticalCenter: parent.verticalCenter }
                        width: 32; height: 32
                        Accessible.name: slot.face.says ? slot.face.says.en : ""
                        readonly property int n: slot.face.open || 0
                        readonly property color ink: slot.face.knocks ? theme.urgHigh : theme.faint
                        PenMark {
                            visible: !!slot.face.hand
                            anchors.fill: parent
                            svg: Pen.handSvg(); turned: false
                            ink: theme.urgCritical; lineWidth: 2
                        }
                        Label {
                            visible: !slot.face.hand && badge.n > 5
                            anchors.centerIn: parent
                            text: badge.n; color: badge.ink; font.pixelSize: 15; font.weight: Font.Medium
                        }
                        PenMark {
                            visible: !slot.face.hand && badge.n > 0 && badge.n <= 5
                            anchors.right: parent.right; anchors.rightMargin: 5; anchors.verticalCenter: parent.verticalCenter
                            width: badge.n > 4 ? 22 : badge.n * 5 + 1; height: 16
                            svg: root.tallySvg(badge.n); turned: false
                            ink: badge.ink; lineWidth: 1.8
                        }
                    }
                    // a row opens its session's page; a main with helpers also folds and unfolds
                    MouseArea {
                        anchors.fill: parent
                        cursorShape: Qt.PointingHandCursor
                        onClicked: {
                            if (slot.s.folded) { const u = Object.assign({}, root.unfolded); u[slot.s.id] = !u[slot.s.id]; root.unfolded = u }
                            root.go({ screen: "session", session_id: slot.s.id })
                        }
                    }
                }
            }
        }
    }

    // ---- the foot: light or dark ----
    Row {
        id: foot
        anchors { right: parent.right; bottom: parent.bottom; rightMargin: 30; bottomMargin: 14 }
        height: 24
        PenMark {
            width: 20; height: 20
            svg: Pen.sketchSvg(theme.dark ? "sun" : "moon"); ink: theme.muted; lineWidth: 1.8
            MouseArea { anchors.fill: parent; cursorShape: Qt.PointingHandCursor; onClicked: root.theme_toggled() }
        }
    }
    Rectangle { anchors.right: parent.right; width: 1; height: parent.height; color: theme.line }
}
