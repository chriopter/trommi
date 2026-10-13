import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// One card of the Desk. Urgency shows as a word on a tile beside the session's name; the options are tiles, the
// agent's pick outlined. An info card is read; a permission request allowed or denied.
Rectangle {
    id: root
    required property var trommi
    required property var theme
    required property var card

    readonly property bool decision: card.kind === "card" && card.card_type !== "info"
    readonly property bool request: card.kind === "request"
    property bool expanded: false
    property var picked: []

    height: content.implicitHeight + 44
    radius: 14
    color: theme.surface
    border.color: theme.line

    function urgencyWord(u) {
        if (u === "critical") return "Blocks work"
        if (u === "high") return "Urgent"
        return ""
    }

    ColumnLayout {
        id: content
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: parent.top
        anchors.margins: 22
        spacing: 10

        // the head: urgency, session, when
        RowLayout {
            Layout.fillWidth: true
            spacing: 8
            Rectangle {
                visible: root.urgencyWord(root.card.urgency) !== ""
                radius: 6
                height: 22
                width: urgency.implicitWidth + 16
                color: root.card.urgency === "critical" ? theme.criticalSoft : theme.highSoft
                Label {
                    id: urgency
                    anchors.centerIn: parent
                    text: (root.card.urgency === "critical" ? "‼ " : "! ") + root.urgencyWord(root.card.urgency)
                    color: root.card.urgency === "critical" ? theme.critical : theme.high
                    font.pixelSize: 12
                    font.weight: Font.DemiBold
                }
            }
            Rectangle {
                visible: root.request
                radius: 6; height: 22; width: reqWord.implicitWidth + 16
                color: theme.highSoft
                Label { id: reqWord; anchors.centerIn: parent; text: "Permission"; color: theme.high; font.pixelSize: 12; font.weight: Font.DemiBold }
            }
            Label {
                text: root.card.session || ""
                color: theme.muted
                font.pixelSize: 13
                font.weight: Font.Medium
                elide: Text.ElideRight
                Layout.fillWidth: true
            }
            Label { text: root.card.ago || ""; color: theme.faint; font.pixelSize: 12 }
        }

        Label {
            text: root.card.title || "Loading …"
            font.family: theme.display
            font.pixelSize: 19
            font.weight: Font.DemiBold
            color: theme.fg
            wrapMode: Text.Wrap
            Layout.fillWidth: true
        }

        Label {
            visible: !!root.card.teaser
            text: root.card.teaser || ""
            color: theme.muted
            wrapMode: Text.Wrap
            Layout.fillWidth: true
        }

        Text {
            id: body
            visible: !!root.card.body && (root.expanded || !root.card.teaser)
            text: root.card.body || ""
            textFormat: Text.MarkdownText
            color: theme.fg
            wrapMode: Text.Wrap
            font.family: theme.sans
            font.pixelSize: 14
            lineHeight: 1.15
            Layout.fillWidth: true
            onLinkActivated: (link) => Qt.openUrlExternally(link)
        }
        Label {
            visible: !!root.card.body && !!root.card.teaser
            text: root.expanded ? "Less" : "More"
            color: theme.accent
            font.pixelSize: 13
            MouseArea { anchors.fill: parent; cursorShape: Qt.PointingHandCursor; onClicked: root.expanded = !root.expanded }
        }
        Label {
            visible: root.card.preview !== undefined && root.card.preview !== ""
            text: root.card.preview || ""
            font.family: "monospace"
            font.pixelSize: 12
            color: theme.muted
            wrapMode: Text.WrapAnywhere
            Layout.fillWidth: true
        }
        Label {
            visible: root.card.attachments > 0
            text: root.card.attachments === 1 ? "1 attachment (open it on the web or the phone)" : root.card.attachments + " attachments (open them on the web or the phone)"
            color: theme.faint
            font.pixelSize: 12
        }

        // the options, as tiles
        Flow {
            Layout.fillWidth: true
            Layout.topMargin: 4
            spacing: 8
            visible: (root.decision || root.request) && root.card.options.length > 0
            Repeater {
                model: root.card.options || []
                delegate: Rectangle {
                    id: tile
                    required property var modelData
                    readonly property bool chosen: root.picked.indexOf(modelData.key) >= 0
                    readonly property bool deny: root.request && modelData.key === "deny"
                    width: Math.min(root.width - 44, Math.max(150, tileCol.implicitWidth + 28))
                    height: tileCol.implicitHeight + 20
                    radius: 10
                    color: chosen ? theme.accentSoft : (tileHover.hovered ? theme.sunken : theme.surface)
                    border.width: modelData.recommended || chosen ? 1.5 : 1
                    border.color: chosen || modelData.recommended ? theme.accent : theme.lineStrong
                    opacity: root.card.answered ? 0.55 : 1
                    HoverHandler { id: tileHover; cursorShape: Qt.PointingHandCursor }
                    ColumnLayout {
                        id: tileCol
                        anchors.left: parent.left
                        anchors.right: parent.right
                        anchors.verticalCenter: parent.verticalCenter
                        anchors.leftMargin: 14
                        anchors.rightMargin: 14
                        spacing: 2
                        RowLayout {
                            spacing: 6
                            Label {
                                text: tile.modelData.label || tile.modelData.key
                                color: tile.deny ? theme.critical : theme.fg
                                font.weight: Font.DemiBold
                            }
                            Label {
                                visible: !!tile.modelData.recommended
                                text: "Recommended"
                                color: theme.accent
                                font.pixelSize: 11
                            }
                        }
                        Text {
                            visible: !!tile.modelData.detail
                            text: tile.modelData.detail || ""
                            textFormat: Text.MarkdownText
                            color: theme.muted
                            font.pixelSize: 12
                            font.family: theme.sans
                            wrapMode: Text.Wrap
                            Layout.maximumWidth: 420
                        }
                    }
                    MouseArea {
                        anchors.fill: parent
                        enabled: !root.card.answered
                        onClicked: {
                            if (root.card.multiple) {
                                let p = root.picked.slice()
                                const at = p.indexOf(tile.modelData.key)
                                if (at >= 0) p.splice(at, 1); else p.push(tile.modelData.key)
                                root.picked = p
                            } else {
                                root.picked = [tile.modelData.key]
                                trommi.answer(root.card.id, tile.modelData.key)
                            }
                        }
                    }
                }
            }
        }

        RowLayout {
            Layout.fillWidth: true
            spacing: 10
            visible: (root.decision && root.card.multiple) || root.card.card_type === "info" || root.card.answered

            Button {
                id: sendMany
                visible: root.decision && root.card.multiple && !root.card.answered
                enabled: root.picked.length > 0
                text: "Send " + root.picked.length
                onClicked: trommi.answer_many(root.card.id, JSON.stringify(root.picked))
                contentItem: Label { text: sendMany.text; color: sendMany.enabled ? theme.accentFg : theme.faint; font.weight: Font.DemiBold; horizontalAlignment: Text.AlignHCenter; verticalAlignment: Text.AlignVCenter }
                background: Rectangle { implicitWidth: 96; implicitHeight: 36; radius: 10; color: sendMany.enabled ? theme.accent : theme.sunken }
            }
            Button {
                id: gotIt
                visible: root.card.card_type === "info" && !root.card.answered
                text: "Got it"
                onClicked: trommi.mark_read(root.card.id)
                contentItem: Label { text: gotIt.text; color: theme.accent; font.weight: Font.DemiBold; horizontalAlignment: Text.AlignHCenter; verticalAlignment: Text.AlignVCenter }
                background: Rectangle { implicitWidth: 96; implicitHeight: 36; radius: 10; color: gotIt.hovered ? theme.accentSoft : theme.surface; border.color: theme.accent }
            }
            Label {
                visible: root.card.answered
                text: root.card.pending ? "Sending …" : "Answered"
                color: theme.muted
                font.pixelSize: 13
            }
        }
    }
}
