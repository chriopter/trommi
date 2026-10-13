import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import "pen.mjs" as Pen

// One open question as the web's Desk shows it (desk.css .inbox-row, ui.mjs deskRow and tiles): the asking session's
// drawing, the urgency's sign, the title in the display face, the pictures as a small fan, and the answers as tiles at
// the right edge, always in the same place. The lead tile (the one the agent advises, or the yes) is filled in the
// session's hue; what knocks keeps its warning colour on it.
Rectangle {
    id: row
    required property var trommi
    required property var theme
    required property var card
    /** The asking session (sessions_json), or null. */
    property var from: null
    signal open()

    readonly property real hue: from ? Pen.hueFor({ id: from.board_id || from.id, mark: from.mark }) : 162
    readonly property bool knock: card.urgency === "high" || card.urgency === "critical"
    readonly property color urg: card.urgency === "critical" ? theme.urgCritical : card.urgency === "high" ? theme.urgHigh : card.urgency === "low" ? theme.urgLow : theme.urgNormal
    readonly property color urgSoft: card.urgency === "critical" ? theme.urgCriticalSoft : card.urgency === "high" ? theme.urgHighSoft : theme.urgNormalSoft
    // (the tiles are the bell's green, as on the web; what knocks keeps its warning colour on the lead)
    readonly property color leadBg: knock ? urg : theme.accent
    readonly property color wash: theme.tone(162, "wash")
    readonly property var tiles: tilesOf(card)

    height: 108
    radius: 6
    color: knock && !theme.dark ? theme.mix(urgSoft, theme.surface, 0.3) : theme.surface
    border.width: 1
    border.color: theme.lineStrong

    // the tiles a card gets (ui.mjs tiles): [{ kind: lead|plain|others|what|choose, key, label, sketch }]
    function tilesOf(c) {
        if (c.answered) return []
        if (c.kind === "request") return [{ kind: "plain", key: "deny", label: "Deny", sketch: "no" }, { kind: "lead", key: "allow", label: "Allow", sketch: "yes" }]
        if (c.card_type === "info") return [{ kind: "what", key: "", label: "", sketch: "what" }, { kind: "lead", key: "read", label: "Got it", sketch: "tick" }]
        const o = c.options || []
        const advised = o.filter(x => x.recommended)
        const bare = o.length === 2 && o.every(x => /^(yes|no|ok|okay|ja|nein)\b/i.test(String(x.label || "").trim()))
        if (o.length === 2 && !c.multiple) {
            const isLead = x => bare ? x === o[0] : advised.length ? x.recommended : false
            return o.slice().sort((a, b) => isLead(a) - isLead(b)).map(x => ({
                kind: isLead(x) ? "lead" : "plain", key: x.key, label: x.label || x.key,
                sketch: bare ? (isLead(x) ? "yes" : "no") : "", advised: !!x.recommended }))
        }
        if (advised.length === 1 && !c.multiple && o.length > 2)
            return [{ kind: "others", key: "", label: "other ways", count: o.length - 1 },
                    { kind: "lead", key: advised[0].key, label: advised[0].label, sketch: "yes", advised: true }]
        return [{ kind: "choose", key: "", label: "Choose", sketch: "choose" }]
    }

    // the sender's drawing, the urgency's sign and the title
    RowLayout {
        anchors { left: parent.left; right: fan.left; verticalCenter: parent.verticalCenter; leftMargin: 20; rightMargin: 16 }
        spacing: 7
        PenMark {
            visible: !!row.from
            Layout.preferredWidth: 22; Layout.preferredHeight: 22; Layout.alignment: Qt.AlignTop; Layout.topMargin: 1
            svg: row.from ? Pen.doodleSvg(row.from.mark || row.from.board_id || row.from.id) : ""
            ink: theme.tone(row.hue, "pen"); lineWidth: 2.1
        }
        Item {
            visible: row.knock || row.card.urgency === "low"
            Layout.preferredWidth: 20; Layout.preferredHeight: 20; Layout.alignment: Qt.AlignTop; Layout.topMargin: 2
            Rectangle { anchors.fill: parent; radius: 10; color: row.card.urgency === "critical" ? "transparent" : (row.knock ? theme.withAlpha(row.urg, 0.14) : "transparent") }
            PenMark {
                anchors.fill: parent; anchors.margins: row.card.urgency === "critical" ? -3 : 2
                svg: row.card.urgency === "critical" ? Pen.handSvg() : Pen.sketchSvg(row.knock ? "knock" : "whenever")
                turned: row.card.urgency !== "critical"
                ink: row.urg; lineWidth: 2
            }
        }
        Text {
            Layout.fillWidth: true
            text: row.card.title || "Loading …"
            font.family: theme.display; font.pixelSize: 18; font.weight: Font.ExtraBold; font.letterSpacing: -0.2
            lineHeight: 1.3
            color: theme.fg
            wrapMode: Text.Wrap
            maximumLineCount: 2
            elide: Text.ElideRight
        }
    }
    MouseArea {
        anchors { left: parent.left; right: tileRow.left; top: parent.top; bottom: parent.bottom }
        cursorShape: Qt.PointingHandCursor
        onClicked: row.open()
    }

    // the pictures as a small fan (the web shows the pictures themselves; this app does not fetch files yet)
    Item {
        id: fan
        anchors { right: tileRow.left; rightMargin: 22; verticalCenter: parent.verticalCenter }
        width: row.card.attachments > 0 ? 58 + Math.min(row.card.attachments - 1, 2) * 18 : 0
        height: 48
        Repeater {
            model: Math.min(row.card.attachments || 0, 3)
            delegate: Rectangle {
                required property int index
                x: index * 18; y: 2 + (index % 2) * 2
                width: 58; height: 44; radius: 4
                rotation: [-3, 2.5, -1.5][index]
                color: theme.sunken
                border.color: theme.fg; border.width: 1
                Column {
                    anchors.centerIn: parent; spacing: 4
                    Repeater { model: 3; Rectangle { width: [34, 26, 30][index]; height: 2; radius: 1; color: theme.lineStrong } }
                }
            }
        }
    }

    // the answers, as tiles at the right edge
    Row {
        id: tileRow
        anchors { right: parent.right; rightMargin: 8; verticalCenter: parent.verticalCenter }
        spacing: 8
        Repeater {
            model: row.tiles
            delegate: Rectangle {
                id: tile
                required property var modelData
                readonly property bool lead: modelData.kind === "lead" || modelData.kind === "choose"
                width: modelData.kind === "choose" ? 124 : 92; height: 92
                radius: 12
                color: lead ? row.leadBg : row.wash
                border.width: hover.hovered && !lead ? 2 : (theme.dark && !lead ? 1 : 0)
                border.color: hover.hovered ? theme.accent : theme.lineStrong
                HoverHandler { id: hover; cursorShape: Qt.PointingHandCursor }
                scale: press.pressed ? 0.97 : 1
                readonly property color ink: lead ? (theme.dark ? theme.accentFg : theme.surface) : theme.cardInk
                Column {
                    anchors.centerIn: parent
                    width: parent.width - 14
                    spacing: 5
                    Label {
                        visible: tile.modelData.kind === "others"
                        anchors.horizontalCenter: parent.horizontalCenter
                        text: "+" + (tile.modelData.count || 0)
                        font.family: theme.display; font.pixelSize: 22; font.weight: Font.ExtraBold
                        color: tile.ink
                    }
                    PenMark {
                        visible: !!tile.modelData.sketch
                        anchors.horizontalCenter: parent.horizontalCenter
                        width: tile.modelData.sketch === "what" ? 44 : 32; height: tile.modelData.sketch === "what" ? 16 : 32
                        svg: tile.modelData.sketch ? Pen.sketchSvg(tile.modelData.sketch) : ""
                        ink: tile.ink; lineWidth: 1.5
                    }
                    Label {
                        visible: tile.modelData.label !== ""
                        width: parent.width
                        horizontalAlignment: Text.AlignHCenter
                        text: tile.modelData.label
                        wrapMode: Text.Wrap
                        maximumLineCount: 3
                        elide: Text.ElideRight
                        font.pixelSize: tile.modelData.kind === "others" ? 12 : 13
                        font.weight: tile.modelData.kind === "others" ? Font.Medium : Font.DemiBold
                        color: tile.ink
                    }
                    // the agent's advice: a stroke of the pen under the advised word
                    PenMark {
                        visible: !!tile.modelData.advised && !tile.modelData.sketch
                        anchors.horizontalCenter: parent.horizontalCenter
                        width: 30; height: 5; turned: false
                        svg: '<svg viewBox="0 0 30 5"><path d="M1 3.2 Q9 1.6 15 2.6 T29 2.2"/></svg>'
                        ink: tile.ink; lineWidth: 1.6
                    }
                }
                MouseArea {
                    id: press
                    anchors.fill: parent
                    onClicked: {
                        const t = tile.modelData
                        if (t.kind === "others" || t.kind === "choose" || t.kind === "what") row.open()
                        else if (t.key === "read") row.trommi.mark_read(row.card.id)
                        else row.trommi.answer(row.card.id, t.key)
                    }
                }
            }
        }
    }
    Label {
        visible: row.card.answered
        anchors { right: parent.right; rightMargin: 24; verticalCenter: parent.verticalCenter }
        text: row.card.pending ? "Sending …" : "Answered"
        color: theme.muted
    }
}
