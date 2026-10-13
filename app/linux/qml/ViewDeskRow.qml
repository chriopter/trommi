import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import "pen.mjs" as Pen

// One row of the Desk, drawn from the app core's view model alone (DeskRowView): the sender's drawing, the sign the
// core names, the title, the small words, a leaf per picture, and the tiles the core lists, in its order. Which
// tiles there are, which leads and what they say is the core's; this file gives each kind its shape and colour.
Rectangle {
    id: root
    required property var theme
    /** DeskRowView */
    required property var row
    /** The sender's short id, for its hue. */
    property string slug: ""
    /** A tile was chosen: the option's key, or "" for a tile that opens the card. */
    signal chosen(string key)

    readonly property var from: row.from || null
    readonly property real hue: from ? Pen.hueFor({ id: slug || from.session_id, mark: from.mark }) : 162
    readonly property bool loud: row.sign === "knock" || row.sign === "block"
    readonly property color urg: row.urgency === "critical" ? theme.urgCritical : row.urgency === "high" ? theme.urgHigh : row.urgency === "low" ? theme.urgLow : theme.urgNormal
    readonly property color urgSoft: row.urgency === "critical" ? theme.urgCriticalSoft : row.urgency === "high" ? theme.urgHighSoft : theme.urgNormalSoft
    readonly property color leadBg: loud ? urg : theme.accent
    readonly property color wash: theme.tone(162, "wash")
    readonly property var tiles: row.tiles || ({ kind: "", tiles: [] })
    /** The shapes at the right edge, in order: the core's tiles, and the fixed ones of each kind. */
    readonly property var shapes: {
        const listed = (tiles.tiles || []).map(t => ({ shape: "answer", tile: t }))
        if (tiles.kind === "read") return [{ shape: "what" }, { shape: "got" }]
        if (tiles.kind === "advised") return [{ shape: "others" }].concat(listed)
        if (tiles.kind === "choose") return [{ shape: "choose" }]
        return listed
    }

    height: 108
    radius: 6
    color: loud && !theme.dark ? theme.mix(urgSoft, theme.surface, 0.3) : theme.surface
    border.width: 1
    border.color: theme.lineStrong

    RowLayout {
        anchors { left: parent.left; right: fan.left; verticalCenter: parent.verticalCenter; leftMargin: 20; rightMargin: 16 }
        spacing: 7
        PenMark {
            visible: !!root.from
            Layout.preferredWidth: 22; Layout.preferredHeight: 22; Layout.alignment: Qt.AlignTop; Layout.topMargin: 1
            svg: root.from ? Pen.doodleSvg(root.from.mark || root.slug || root.from.session_id) : ""
            ink: theme.tone(root.hue, "pen"); lineWidth: 2.1
        }
        Item {
            visible: root.row.sign !== "" && root.row.sign !== undefined
            Layout.preferredWidth: 20; Layout.preferredHeight: 20; Layout.alignment: Qt.AlignTop; Layout.topMargin: 2
            Accessible.name: root.row.sign_says ? root.row.sign_says.en : ""
            Rectangle { anchors.fill: parent; radius: 10; color: root.row.sign === "knock" ? theme.withAlpha(root.urg, 0.14) : "transparent" }
            PenMark {
                anchors.fill: parent; anchors.margins: root.row.sign === "block" ? -3 : 2
                svg: root.row.sign === "block" ? Pen.handSvg() : root.row.sign ? Pen.sketchSvg(root.row.sign) : ""
                turned: root.row.sign !== "block"
                ink: root.urg; lineWidth: 2
            }
        }
        ColumnLayout {
            Layout.fillWidth: true
            spacing: 3
            // a card not read on this device yet waits quietly
            Rectangle { visible: !!root.row.loading; Layout.preferredWidth: 220; Layout.preferredHeight: 12; radius: 6; color: theme.sunken }
            Text {
                visible: !root.row.loading
                Layout.fillWidth: true
                text: root.row.title || ""
                font.family: theme.display; font.pixelSize: 18; font.weight: Font.ExtraBold; font.letterSpacing: -0.2
                lineHeight: 1.3
                color: theme.fg
                wrapMode: Text.Wrap
                maximumLineCount: 2
                elide: Text.ElideRight
            }
            Text {
                visible: text !== ""
                Layout.fillWidth: true
                text: (root.row.notes || []).map(n => n.en).concat(root.row.words ? [root.row.words] : []).join(" · ")
                color: theme.muted; font.family: theme.sans; font.pixelSize: 13
                elide: Text.ElideRight
                maximumLineCount: 1
            }
        }
    }

    // a leaf per picture the core names (the pictures themselves come with the files)
    Item {
        id: fan
        anchors { right: tileRow.left; rightMargin: 22; verticalCenter: parent.verticalCenter }
        readonly property var media: root.row.media || []
        width: media.length > 0 ? 58 + (media.length - 1) * 18 : 0
        height: 48
        Accessible.name: root.row.media_says ? root.row.media_says.en : ""
        Repeater {
            model: fan.media
            delegate: Rectangle {
                required property int index
                x: index * 18; y: 2 + (index % 2) * 2
                z: -index
                width: 58; height: 44; radius: 4
                rotation: [-3, 2.5, -1.5][index % 3]
                color: theme.sunken
                border.color: theme.fg; border.width: 1
                Column {
                    anchors.centerIn: parent; spacing: 4
                    Repeater { model: 3; Rectangle { required property int index; width: [34, 26, 30][index]; height: 2; radius: 1; color: theme.lineStrong } }
                }
            }
        }
    }

    Row {
        id: tileRow
        anchors { right: parent.right; rightMargin: 8; verticalCenter: parent.verticalCenter }
        spacing: 8
        Repeater {
            model: root.shapes
            delegate: Rectangle {
                id: tile
                required property var modelData
                readonly property var t: modelData.tile || null
                readonly property string shape: modelData.shape
                readonly property bool lead: shape === "got" || shape === "choose" || (!!t && t.lead)
                readonly property string sketch: shape === "what" ? "what" : shape === "got" ? "tick" : shape === "choose" ? "choose" : (t && (t.thumbs || t.label === "") ? t.thumb : (t && t.lead && root.tiles.kind === "advised" ? "yes" : ""))
                readonly property string label: shape === "got" ? "Got it" : shape === "choose" ? "Choose" : shape === "others" ? "other ways" : t ? t.label : ""
                width: shape === "choose" ? 124 : 92; height: 92
                radius: 12
                color: lead ? root.leadBg : root.wash
                border.width: hover.hovered && !lead ? 2 : (theme.dark && !lead ? 1 : 0)
                border.color: hover.hovered ? theme.accent : theme.lineStrong
                Accessible.name: t ? t.says.en : (root.tiles.says ? root.tiles.says.en : label)
                HoverHandler { id: hover; cursorShape: Qt.PointingHandCursor }
                readonly property color ink: lead ? (theme.dark ? theme.accentFg : theme.surface) : theme.cardInk
                Column {
                    anchors.centerIn: parent
                    width: parent.width - 14
                    spacing: 5
                    Label {
                        visible: tile.shape === "others"
                        anchors.horizontalCenter: parent.horizontalCenter
                        text: "+" + (root.tiles.others || 0)
                        font.family: theme.display; font.pixelSize: 22; font.weight: Font.ExtraBold
                        color: tile.ink
                    }
                    PenMark {
                        visible: tile.sketch !== ""
                        anchors.horizontalCenter: parent.horizontalCenter
                        width: tile.sketch === "what" ? 44 : 32; height: tile.sketch === "what" ? 16 : 32
                        svg: tile.sketch !== "" ? Pen.sketchSvg(tile.sketch) : ""
                        ink: tile.ink; lineWidth: 1.5
                    }
                    Label {
                        visible: tile.label !== ""
                        width: parent.width
                        horizontalAlignment: Text.AlignHCenter
                        text: tile.label
                        wrapMode: Text.Wrap
                        maximumLineCount: 3
                        elide: Text.ElideRight
                        font.pixelSize: tile.shape === "others" || (tile.t && tile.t.small) ? 12 : 13
                        font.weight: tile.shape === "others" ? Font.Medium : Font.DemiBold
                        color: tile.ink
                    }
                    // the agent's advice: a stroke of the pen under the advised word
                    PenMark {
                        visible: !!tile.t && tile.t.advised && tile.sketch === ""
                        anchors.horizontalCenter: parent.horizontalCenter
                        width: 30; height: 5; turned: false
                        svg: '<svg viewBox="0 0 30 5"><path d="M1 3.2 Q9 1.6 15 2.6 T29 2.2"/></svg>'
                        ink: tile.ink; lineWidth: 1.6
                    }
                }
                MouseArea { anchors.fill: parent; onClicked: root.chosen(tile.t ? tile.t.key : "") }
            }
        }
    }
}
