import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// One row of a conversation, drawn from the view model alone (`TalkRow`): `kind` says what it is, and the fields it
// needs are filled. His words on the right, the agent's on the left, a quiet line for what happened, an open
// question where it was asked (a tap opens it), a turn's trail as its head and step.
Item {
    id: root
    required property var theme
    required property var row
    /** Another page: an `Event::Focus` screen. */
    signal open(var screen)

    readonly property bool mine: row.kind === "user" || row.kind === "note"
    implicitHeight: col.implicitHeight + (row.cont ? 2 : 10)

    ColumnLayout {
        id: col
        anchors { left: parent.left; right: parent.right; top: parent.top; topMargin: root.row.cont ? 2 : 10 }
        spacing: 4

        // a new day begins here
        Label {
            visible: !!root.row.day
            Layout.alignment: Qt.AlignHCenter
            Layout.bottomMargin: 6
            text: root.row.day ? root.row.day.en : ""
            color: theme.faint; font.pixelSize: 12; font.weight: Font.DemiBold
        }
        // what he did with a card along with these words
        Label {
            visible: !!root.row.deed
            Layout.alignment: root.mine ? Qt.AlignRight : Qt.AlignLeft
            text: root.row.deed ? root.row.deed.label.en + (root.row.deed.about ? " · " + root.row.deed.about : "") : ""
            color: theme.faint; font.pixelSize: 12
        }

        // the helper (its agent's subagent) that wrote the words below, when it was one
        Label {
            visible: !!root.row.helper && !root.row.cont
            Layout.alignment: Qt.AlignLeft
            text: root.row.helper || ""
            color: theme.muted; font.pixelSize: 12; font.weight: Font.DemiBold
        }
        // words: his bubble, the agent's text, a note
        Rectangle {
            visible: ["user", "agent", "note", "newer"].indexOf(root.row.kind) >= 0
            Layout.alignment: root.mine ? Qt.AlignRight : Qt.AlignLeft
            Layout.maximumWidth: col.width * 0.82
            implicitWidth: Math.min(col.width * 0.82, words.implicitWidth + 28)
            implicitHeight: words.implicitHeight + 18
            radius: 14
            color: root.row.kind === "user" ? theme.withAlpha(theme.accent, 0.14) : root.row.kind === "note" ? theme.sunken : "transparent"
            border.width: root.row.kind === "agent" ? 0 : 1
            border.color: theme.line
            opacity: root.row.pending ? 0.6 : 1
            Label {
                id: words
                anchors { left: parent.left; right: parent.right; verticalCenter: parent.verticalCenter; leftMargin: 14; rightMargin: 14 }
                text: root.row.text
                textFormat: root.row.kind === "agent" ? Text.MarkdownText : Text.PlainText
                wrapMode: Text.Wrap
                color: theme.fg; font.pixelSize: 14
                onLinkActivated: (link) => Qt.openUrlExternally(link)
            }
        }

        // one quiet line for what happened to a card
        Label {
            visible: root.row.kind === "event" && !!root.row.event
            Layout.fillWidth: true
            wrapMode: Text.Wrap
            text: root.row.event ? [root.row.event.label.en, root.row.event.about, root.row.event.text].filter(s => s).join(" · ") : ""
            color: theme.muted; font.pixelSize: 13
            MouseArea {
                anchors.fill: parent
                enabled: !!root.row.card
                cursorShape: enabled ? Qt.PointingHandCursor : Qt.ArrowCursor
                onClicked: root.open({ screen: "card", card_id: root.row.card.id })
            }
        }

        // an open question where it was asked
        Rectangle {
            visible: root.row.kind === "ask" && !!root.row.row
            Layout.fillWidth: true
            implicitHeight: askTitle.implicitHeight + 28
            radius: 12
            color: theme.surface
            border.width: 1; border.color: theme.lineStrong
            Label {
                id: askTitle
                anchors { left: parent.left; right: parent.right; verticalCenter: parent.verticalCenter; margins: 16 }
                text: root.row.row ? root.row.row.title : ""
                wrapMode: Text.Wrap
                color: theme.fg; font.pixelSize: 15; font.weight: Font.Bold
            }
            MouseArea {
                anchors.fill: parent
                cursorShape: Qt.PointingHandCursor
                onClicked: root.open({ screen: "card", card_id: root.row.row.id })
            }
        }

        // a turn's trail: its head, and the step it is at
        Column {
            visible: root.row.kind === "work" && !!root.row.work
            Layout.fillWidth: true
            spacing: 2
            Label { text: root.row.work ? root.row.work.head.en + (root.row.work.sum ? " · " + root.row.work.sum.en : "") : ""; color: theme.muted; font.pixelSize: 13; font.weight: Font.DemiBold }
            Label { visible: !!root.row.work && root.row.work.now !== ""; text: root.row.work ? root.row.work.now : ""; color: theme.faint; font.family: theme.mono; font.pixelSize: 12 }
        }

        // the cards it carries
        Flow {
            visible: (root.row.chips || []).length > 0
            Layout.fillWidth: true
            spacing: 6
            Repeater {
                model: root.row.chips || []
                delegate: Rectangle {
                    required property var modelData
                    width: chip.implicitWidth + 16; height: 24; radius: 12
                    color: theme.sunken
                    Label { id: chip; anchors.centerIn: parent; text: modelData.title; color: theme.fg; font.pixelSize: 12 }
                    MouseArea { anchors.fill: parent; cursorShape: Qt.PointingHandCursor; onClicked: root.open({ screen: "card", card_id: modelData.id }) }
                }
            }
        }
    }
}
