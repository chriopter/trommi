import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import "pen.mjs" as Pen

// The desktop sidebar as the web draws it (sidebar.css): the desk switch at the top, then per desk its divider (the
// desk's drawing, its name, a pen rule, how many questions wait there) and its main sessions as cards: the session's
// drawing in its hue, the crown of the desk, its name, its link when it is not live, its questions as a tally, and
// under the card the edges of its subs, each in its own hue. The subs unfold under their main session when its edges
// are clicked.
Rectangle {
    id: root
    required property var trommi
    required property var theme
    required property var board
    signal home()
    color: theme.bg

    property var unfolded: ({})
    readonly property var desks: board.desks || []
    readonly property var sessions: board.sessions || []

    function hueOf(s) { return Pen.hueFor({ id: s.board_id || s.id, mark: s.mark }) }
    function markOf(s) { return Pen.doodleSvg(s.mark || s.board_id || s.id) }
    function linkSign(s) {
        switch (s.link) {
        case "gone": return { sign: "plug", word: "gone" }
        case "cut": return { sign: "ear-off", word: "cut off" }
        case "asleep": return { sign: "ear-later", word: "not listening" }
        case "oncall": return { sign: "ear-later", word: "on its next step" }
        default: return null
        }
    }
    // the tally: a stroke for each question up to four, the fifth across them (ui.mjs tallySvg)
    function tallySvg(n) {
        const T = ['M3.9 2.2Q2.6 7.6 2.5 13.9', 'M8.6 1.6Q8.4 8.4 7.3 13.2', 'M13.7 2.6Q12.5 7.2 12.6 14.2', 'M18.6 1.9Q18.3 8.6 17.2 13.5']
        const w = n > 4 ? 22 : n * 5 + 1
        let s = '<svg viewBox="0 0 ' + w + ' 16">'
        for (let i = 0; i < Math.min(n, 4); i++) s += '<path d="' + T[i] + '"/>'
        if (n > 4) s += '<path d="M.9 11.8Q10.5 7.6 21.1 3.5"/>'
        return s + '</svg>'
    }
    readonly property var rows: {
        const out = []
        for (const d of desks) {
            const mains = sessions.filter(s => s.desk === d.id && s.depth === 0)
            if (!mains.length) continue
            out.push({ kind: "desk", desk: d, first: out.length === 0 })
            for (const m of mains) {
                out.push({ kind: "session", s: m })
                if (unfolded[m.id]) {
                    const at = sessions.indexOf(m)
                    for (let i = at + 1; i < sessions.length && sessions[i].depth > 0; i++) out.push({ kind: "session", s: sessions[i] })
                }
            }
        }
        return out
    }

    // ---- the desk switch ----
    RowLayout {
        id: head
        anchors { left: parent.left; right: parent.right; top: parent.top; leftMargin: 18; rightMargin: 24; topMargin: 15 }
        height: 40
        spacing: 12
        PenMark {
            Layout.preferredWidth: 34; Layout.preferredHeight: 34
            svg: Pen.sketchSvg("desk"); ink: theme.fg; lineWidth: 1.7
        }
        Label {
            text: "All desks"
            font.family: theme.display; font.pixelSize: 23; font.weight: Font.ExtraBold
            font.letterSpacing: -0.4
            color: theme.fg
            Layout.fillWidth: true
        }
        PenMark {
            Layout.preferredWidth: 14; Layout.preferredHeight: 14
            svg: Pen.sketchSvg("unfold"); ink: theme.muted; lineWidth: 2
        }
    }
    MouseArea { anchors.fill: head; cursorShape: Qt.PointingHandCursor; onClicked: { root.trommi.select_session(""); root.home() } }

    ListView {
        id: list
        anchors { left: parent.left; right: parent.right; top: head.bottom; bottom: foot.top; leftMargin: 16; rightMargin: 9; topMargin: 22 }
        clip: true
        spacing: 0
        model: root.rows
        ScrollBar.vertical: ScrollBar { policy: ScrollBar.AsNeeded; width: 6 }
        delegate: Loader {
            id: slot
            required property var modelData
            width: list.width - 2
            sourceComponent: modelData.kind === "desk" ? deskDivider : sessionCard
            property var entry: modelData
        }
    }

    Component {
        id: deskDivider
        Item {
            readonly property var d: parent ? parent.entry.desk : ({})
            readonly property bool first: parent ? parent.entry.first : true
            height: first ? 32 : 52
            width: parent ? parent.width : 0
            RowLayout {
                anchors { left: parent.left; right: parent.right; bottom: parent.bottom; leftMargin: 6; rightMargin: 6; bottomMargin: 14 }
                spacing: 6
                PenMark { Layout.preferredWidth: 18; Layout.preferredHeight: 18; svg: Pen.sketchSvg("desk"); ink: theme.muted; lineWidth: 1.7 }
                Label { text: d.name || ""; color: theme.muted; font.pixelSize: 13; font.weight: Font.DemiBold }
                PenMark {
                    Layout.fillWidth: true; Layout.preferredHeight: 6; Layout.leftMargin: 4; Layout.rightMargin: 4
                    svg: '<svg viewBox="0 0 200 6"><path d="M1 3.4 Q40 2.2 90 3.1 T199 2.6"/></svg>'; ink: theme.lineStrong; lineWidth: 1.6; turned: false
                }
                Label {
                    visible: (d.waiting || 0) > 0
                    text: d.waiting || ""
                    color: theme.urgHigh; font.pixelSize: 12; font.weight: Font.Bold
                }
            }
        }
    }

    Component {
        id: sessionCard
        Item {
            id: card
            readonly property var s: parent ? parent.entry.s : ({})
            readonly property real hue: root.hueOf(s)
            readonly property var link: root.linkSign(s)
            readonly property bool current: trommi.selected === s.id
            readonly property int subs: (s.helpers || []).length
            readonly property bool sub: s.depth > 0
            readonly property bool folded: subs > 0 && !root.unfolded[s.id]
            width: parent ? parent.width : 0
            // its questions and, folded, its subs' questions with them (the web's badge of a folded main)
            readonly property int waiting: (s.waiting || 0) + (folded ? (s.helpers || []).reduce((n, h) => n + (h.waiting || 0), 0) : 0)
            height: entry.height + (folded ? 12 + Math.min(subs, 6) * 2.4 : 0) + (sub ? 6 : 34)

            // the subs' edges under the card, folded: one per sub, in its own hue (ui.mjs edgeQuirk)
            Repeater {
                model: card.folded ? (card.s.helpers || []).slice(0, 6) : []
                delegate: PenMark {
                    required property var modelData
                    required property int index
                    readonly property var quirk: Pen.edgeQuirk(modelData.board_id)
                    readonly property real h: root.hueOf(modelData)
                    readonly property color ground: theme.dark ? theme.mix(theme.tone(h, "edge"), theme.bg, .3) : theme.tone(h, "edge")
                    x: 7 + Number(quirk.dx)
                    y: entry.height - 10 + (Math.min(card.subs, 6) - index) * 2.4
                    z: -(Math.min(card.subs, 6) - index)
                    width: entry.width - 14; height: 16
                    rotation: Number(quirk.tilt)
                    turned: false
                    svg: quirk.svg
                    ink: theme.dark ? theme.mix(theme.tone(h, "pen"), theme.bg, .45) : theme.tone(h, "pen"); lineWidth: 1.3
                    fills: ({ "e-w": ground, "e-n": ground })
                    MouseArea { anchors.fill: parent; cursorShape: Qt.PointingHandCursor; onClicked: { const u = Object.assign({}, root.unfolded); u[card.s.id] = true; root.unfolded = u } }
                }
            }

            Rectangle {
                id: entry
                x: card.sub ? 20 : 0
                width: card.width - x
                height: card.sub ? 42 : 51
                radius: card.sub ? 12 : 14
                color: theme.surface
                border.width: card.current ? 1.5 : 1
                border.color: card.current ? theme.fg : theme.lineStrong
                HoverHandler { id: hover; cursorShape: Qt.PointingHandCursor }
                scale: hover.hovered ? 1.012 : 1
                Behavior on scale { NumberAnimation { duration: 90 } }

                // the session's drawing in its hue, the crown above it
                PenMark {
                    id: mark
                    x: card.sub ? 10 : 12; anchors.verticalCenter: parent.verticalCenter
                    width: card.sub ? 28 : 38; height: width
                    svg: root.markOf(card.s)
                    // (an offline session's drawing in grey, faded: sidebar.css .agent-avatar.is-offline)
                    ink: card.s.online ? theme.tone(card.hue, "pen") : theme.grey(theme.tone(card.hue, "pen"))
                    opacity: card.s.online ? 1 : 0.6
                    lineWidth: 2
                }
                PenMark {
                    visible: !!card.s.crown
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
                        text: card.s.name || ""
                        color: card.s.online ? theme.fg : theme.muted
                        font.pixelSize: card.sub ? 14 : 17; font.weight: card.sub ? Font.DemiBold : Font.Bold
                        elide: Text.ElideRight
                        Layout.fillWidth: true
                    }
                    RowLayout {
                        visible: !!card.link
                        spacing: 3
                        PenMark { Layout.preferredWidth: 16; Layout.preferredHeight: 16; svg: card.link ? Pen.sketchSvg(card.link.sign) : ""; ink: theme.faint; lineWidth: 1.9 }
                        Label { text: card.link ? card.link.word : ""; color: theme.faint; font.pixelSize: 12 }
                    }
                }
                // its open questions: a tally, a figure above five; the raised hand when it is stopped
                Item {
                    id: badge
                    anchors { right: parent.right; rightMargin: 10; verticalCenter: parent.verticalCenter }
                    width: 32; height: 32
                    PenMark {
                        visible: !!card.s.blocked
                        anchors.fill: parent
                        svg: Pen.handSvg(); turned: false
                        ink: theme.urgCritical; lineWidth: 2
                    }
                    Label {
                        visible: !card.s.blocked && card.waiting > 5
                        anchors.centerIn: parent
                        text: card.waiting || ""; color: theme.faint; font.pixelSize: 15; font.weight: Font.Medium
                    }
                    PenMark {
                        visible: !card.s.blocked && card.waiting > 0 && card.waiting <= 5
                        anchors.right: parent.right; anchors.rightMargin: 5; anchors.verticalCenter: parent.verticalCenter
                        width: card.waiting > 4 ? 22 : card.waiting * 5 + 1; height: 16
                        svg: root.tallySvg(card.waiting || 0); turned: false
                        ink: theme.faint; lineWidth: 1.8
                    }
                }
                MouseArea {
                    anchors.fill: parent
                    onClicked: trommi.select_session(card.s.id)
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
            MouseArea { anchors.fill: parent; cursorShape: Qt.PointingHandCursor; onClicked: trommi.set_theme(theme.dark ? "light" : "dark") }
        }
    }
    Rectangle { anchors.right: parent.right; width: 1; height: parent.height; color: theme.line }
}
