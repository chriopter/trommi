import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import "pen.mjs" as Pen

// A card's own page: who asks, the question in the display face, its teaser and body, and every option as a tile
// in the asking session's hue (the agent's advice outlined with the pen). Several options: picked, then sent.
Item {
    id: root
    required property var trommi
    required property var theme
    required property var card
    property var from: null
    signal back()

    property var picked: []
    readonly property real hue: from ? Pen.hueFor({ id: from.board_id || from.id, mark: from.mark }) : 162
    readonly property bool decision: card.kind === "card" && card.card_type !== "info"
    readonly property bool request: card.kind === "request"
    readonly property color urg: card.urgency === "critical" ? theme.urgCritical : card.urgency === "high" ? theme.urgHigh : theme.urgNormal

    Flickable {
        id: flick
        anchors.fill: parent
        contentHeight: col.implicitHeight + 120
        clip: true
        ScrollBar.vertical: ScrollBar { policy: ScrollBar.AsNeeded }

        ColumnLayout {
            id: col
            x: (flick.width - width) / 2
            y: 40
            width: Math.min(720, flick.width - 64)
            spacing: 14

            // back, and who asks
            RowLayout {
                spacing: 10
                Label {
                    text: "← Desk"; color: theme.muted; font.pixelSize: 14; font.weight: Font.DemiBold
                    MouseArea { anchors.fill: parent; cursorShape: Qt.PointingHandCursor; onClicked: root.back() }
                }
                Item { Layout.fillWidth: true }
                PenMark {
                    visible: !!root.from
                    Layout.preferredWidth: 22; Layout.preferredHeight: 22
                    svg: root.from ? Pen.doodleSvg(root.from.mark || root.from.board_id || root.from.id) : ""
                    ink: theme.tone(root.hue, "pen"); lineWidth: 2.1
                }
                Label { text: root.card.session || ""; color: theme.tone(root.hue, "pen"); font.weight: Font.DemiBold }
                Label { text: root.card.ago ? "· " + root.card.ago : ""; color: theme.faint; font.pixelSize: 13 }
            }

            Rectangle {
                visible: root.card.urgency === "high" || root.card.urgency === "critical" || root.request
                Layout.preferredHeight: 26
                Layout.preferredWidth: tabText.implicitWidth + 22
                radius: 6
                color: root.request ? theme.urgHigh : root.urg
                Label {
                    id: tabText
                    anchors.centerIn: parent
                    text: root.request ? "PERMISSION" : root.card.urgency === "critical" ? "BLOCKS WORK" : "KNOCKS"
                    color: theme.surface; font.pixelSize: 12; font.weight: Font.DemiBold; font.letterSpacing: 0.8
                }
            }

            Text {
                Layout.fillWidth: true
                text: root.card.title || ""
                font.family: theme.display; font.pixelSize: 34; font.weight: Font.ExtraBold; font.letterSpacing: -0.8
                lineHeight: 1.08
                color: theme.fg
                wrapMode: Text.Wrap
            }
            Text {
                visible: !!root.card.teaser
                Layout.fillWidth: true
                text: root.card.teaser || ""
                font.family: theme.sans; font.pixelSize: 18; lineHeight: 1.3
                color: theme.cardInk
                wrapMode: Text.Wrap
            }
            Text {
                visible: !!root.card.body
                Layout.fillWidth: true
                Layout.topMargin: 4
                text: root.card.body || ""
                textFormat: Text.MarkdownText
                font.family: theme.sans; font.pixelSize: 16; lineHeight: 1.35
                color: theme.fg
                wrapMode: Text.Wrap
                onLinkActivated: (link) => Qt.openUrlExternally(link)
            }
            Text {
                visible: !!root.card.preview
                Layout.fillWidth: true
                text: root.card.preview || ""
                font.family: theme.mono; font.pixelSize: 13
                color: theme.muted
                wrapMode: Text.WrapAnywhere
            }
            Label {
                visible: root.card.attachments > 0
                text: root.card.attachments === 1 ? "1 picture or file: open it on the web or the phone" : root.card.attachments + " pictures or files: open them on the web or the phone"
                color: theme.faint; font.pixelSize: 13
            }

            // the options
            Repeater {
                model: (root.decision || root.request) && !root.card.answered ? (root.card.options || []) : []
                delegate: Rectangle {
                    id: opt
                    required property var modelData
                    readonly property bool chosen: root.picked.indexOf(modelData.key) >= 0
                    Layout.fillWidth: true
                    Layout.topMargin: index === 0 ? 10 : 0
                    required property int index
                    implicitHeight: optCol.implicitHeight + 30
                    radius: 12
                    color: chosen ? theme.tone(root.hue, "edge") : theme.tone(root.hue, "wash")
                    border.width: modelData.recommended || chosen || hover.hovered ? 2 : 0
                    border.color: theme.tone(root.hue, "pen")
                    HoverHandler { id: hover; cursorShape: Qt.PointingHandCursor }
                    ColumnLayout {
                        id: optCol
                        anchors { left: parent.left; right: parent.right; verticalCenter: parent.verticalCenter; leftMargin: 18; rightMargin: 18 }
                        spacing: 4
                        RowLayout {
                            spacing: 10
                            Label {
                                text: opt.modelData.label || opt.modelData.key
                                font.family: theme.display; font.pixelSize: 19; font.weight: Font.ExtraBold
                                color: opt.modelData.key === "deny" ? theme.deny : theme.fg
                            }
                            Rectangle {
                                visible: !!opt.modelData.recommended
                                height: 20; width: rec.implicitWidth + 14; radius: 10
                                color: theme.tone(root.hue, "pen")
                                Label { id: rec; anchors.centerIn: parent; text: "Advised"; color: theme.surface; font.pixelSize: 11; font.weight: Font.DemiBold }
                            }
                        }
                        Text {
                            visible: !!opt.modelData.detail
                            Layout.fillWidth: true
                            text: opt.modelData.detail || ""
                            textFormat: Text.MarkdownText
                            font.family: theme.sans; font.pixelSize: 14
                            color: theme.cardInk
                            wrapMode: Text.Wrap
                        }
                    }
                    MouseArea {
                        anchors.fill: parent
                        onClicked: {
                            if (root.card.multiple) {
                                const p = root.picked.slice(), at = p.indexOf(opt.modelData.key)
                                if (at >= 0) p.splice(at, 1); else p.push(opt.modelData.key)
                                root.picked = p
                            } else {
                                root.trommi.answer(root.card.id, opt.modelData.key)
                                root.back()
                            }
                        }
                    }
                }
            }

            RowLayout {
                Layout.topMargin: 10
                spacing: 10
                Button {
                    id: sendMany
                    visible: root.decision && root.card.multiple && !root.card.answered
                    enabled: root.picked.length > 0
                    text: root.picked.length ? "Send " + root.picked.length : "Pick one or more"
                    onClicked: { root.trommi.answer_many(root.card.id, JSON.stringify(root.picked)); root.back() }
                    contentItem: Label { text: sendMany.text; color: sendMany.enabled ? theme.accentFg : theme.faint; font.weight: Font.DemiBold; horizontalAlignment: Text.AlignHCenter; verticalAlignment: Text.AlignVCenter }
                    background: Rectangle { implicitWidth: 140; implicitHeight: 44; radius: 12; color: sendMany.enabled ? theme.tone(root.hue, "pen") : theme.sunken }
                }
                Button {
                    id: gotIt
                    visible: root.card.card_type === "info" && !root.card.answered
                    text: "Got it"
                    onClicked: { root.trommi.mark_read(root.card.id); root.back() }
                    contentItem: Row {
                        spacing: 8
                        PenMark { width: 22; height: 22; anchors.verticalCenter: parent.verticalCenter; svg: Pen.sketchSvg("tick"); ink: theme.surface; lineWidth: 2 }
                        Label { anchors.verticalCenter: parent.verticalCenter; text: gotIt.text; color: theme.surface; font.weight: Font.DemiBold }
                    }
                    background: Rectangle { implicitWidth: 140; implicitHeight: 44; radius: 12; color: theme.tone(root.hue, "pen") }
                }
                Label {
                    visible: !!root.card.answered
                    text: root.card.pending ? "Sending …" : "Answered"
                    color: theme.muted
                }
            }
        }
    }
}
