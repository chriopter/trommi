import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import "pen.mjs" as Pen

// One card's page, drawn from the app core's view model alone (ViewModel.cards[id]): who it comes from, its words,
// the answers' column as the core says it, and its comments. A tap becomes an action (`act`); what it means is
// the core's. The only state of this file's own: the options ticked on a card that takes several, and the words
// being typed.
Item {
    id: root
    required property var theme
    /** ViewModel.cards[id] */
    required property var card
    /** An `Action` (contract spelling, `do` and its fields). */
    signal act(var action)
    /** Another page: an `Event::Focus` screen ({ screen: "desk" }, { screen: "card", card_id }). */
    signal go(var screen)

    property var ticked: ({})
    onCardChanged: if (!card || card.id !== ticked.__id) ticked = ({ __id: card ? card.id : "" })
    readonly property var answer: card.answer || ({ mode: "" })
    readonly property var place: card.place || null
    /** The answers' column offers answers (the core's mode): then the card can also be put off or handed back. */
    readonly property bool canAnswer: answer.mode === "options" || answer.mode === "read"

    Flickable {
        id: flick
        anchors.fill: parent
        contentHeight: col.implicitHeight + 80
        clip: true
        ScrollBar.vertical: ScrollBar { policy: ScrollBar.AsNeeded }

        ColumnLayout {
            id: col
            x: Math.max(32, (flick.width - width) / 2)
            y: 40
            width: Math.min(760, flick.width - 64)
            spacing: 14

            // the way back, and the card's place among what waits
            RowLayout {
                Layout.fillWidth: true
                Label {
                    text: "‹ Desk"
                    color: theme.accent; font.pixelSize: 14; font.weight: Font.DemiBold
                    MouseArea { anchors.fill: parent; cursorShape: Qt.PointingHandCursor; onClicked: root.go({ screen: "desk" }) }
                }
                Item { Layout.fillWidth: true }
                Label {
                    visible: !!root.place && !!root.place.prev
                    text: "‹"; color: theme.muted; font.pixelSize: 18
                    MouseArea { anchors.fill: parent; cursorShape: Qt.PointingHandCursor; onClicked: root.go({ screen: "card", card_id: root.place.prev }) }
                }
                Label { visible: !!root.place; text: root.place ? root.place.says.en : ""; color: theme.muted; font.pixelSize: 13 }
                Label {
                    visible: !!root.place && !!root.place.next
                    text: "›"; color: theme.muted; font.pixelSize: 18
                    MouseArea { anchors.fill: parent; cursorShape: Qt.PointingHandCursor; onClicked: root.go({ screen: "card", card_id: root.place.next }) }
                }
            }

            // who it comes from, its number, the knock
            RowLayout {
                Layout.fillWidth: true
                spacing: 8
                PenMark {
                    visible: !!root.card.from
                    Layout.preferredWidth: 22; Layout.preferredHeight: 22
                    svg: root.card.from ? Pen.doodleSvg(root.card.from.mark || root.card.from.session_id) : ""
                    ink: theme.muted; lineWidth: 1.8
                }
                Label {
                    text: root.card.from ? root.card.from.path.join(" › ") : ""
                    color: theme.muted; font.pixelSize: 13
                    Layout.fillWidth: true; elide: Text.ElideRight
                    MouseArea {
                        anchors.fill: parent
                        enabled: !!root.card.from
                        cursorShape: Qt.PointingHandCursor
                        onClicked: root.go({ screen: "session", session_id: root.card.from.session_id })
                    }
                }
                Label { text: root.card.nr ? root.card.nr.en : ""; color: theme.faint; font.pixelSize: 13 }
            }
            // a helper's card: the helper (its agent's subagent) it comes from
            Label {
                visible: !!(root.card.from && root.card.from.helper)
                text: root.card.from && root.card.from.helper ? root.card.from.helper.en : ""
                color: theme.fg; font.pixelSize: 12; font.weight: Font.DemiBold
                leftPadding: 8; rightPadding: 8; topPadding: 2; bottomPadding: 2
                background: Rectangle { radius: height / 2; color: theme.sunken }
            }
            Label {
                visible: !!root.card.knock
                text: root.card.knock ? root.card.knock.en : ""
                color: theme.urgHigh; font.pixelSize: 13; font.weight: Font.Bold
            }
            Rectangle {
                visible: !!root.card.work
                Layout.fillWidth: true
                implicitHeight: workCol.implicitHeight + 16
                radius: 8; color: theme.sunken
                Column {
                    id: workCol
                    anchors { left: parent.left; right: parent.right; verticalCenter: parent.verticalCenter; margins: 12 }
                    Label { text: root.card.work ? root.card.work.says.en : ""; color: theme.fg; font.pixelSize: 13; font.weight: Font.DemiBold }
                    Label { text: root.card.work ? root.card.work.words.en : ""; color: theme.muted; font.pixelSize: 13 }
                }
            }

            Label {
                Layout.fillWidth: true
                text: root.card.title || ""
                wrapMode: Text.Wrap
                font.family: theme.display; font.pixelSize: 30; font.weight: Font.ExtraBold; font.letterSpacing: -0.6
                color: theme.fg
            }
            Label {
                visible: (root.card.notes || []).length > 0
                text: (root.card.notes || []).map(n => n.en).join(" · ")
                color: theme.faint; font.pixelSize: 13
            }
            Label {
                visible: (root.card.text || "") !== ""
                Layout.fillWidth: true
                text: root.card.text || ""
                textFormat: Text.MarkdownText
                wrapMode: Text.Wrap
                color: theme.fg; font.pixelSize: 15
                onLinkActivated: (link) => Qt.openUrlExternally(link)
            }

            // ---- the answers' column, as the core says it -------------------------------------------------------
            // the options
            Flow {
                visible: root.answer.mode === "options"
                Layout.fillWidth: true
                Layout.topMargin: 8
                spacing: 10
                Repeater {
                    model: root.answer.mode === "options" ? (root.card.options || []) : []
                    delegate: Rectangle {
                        id: tile
                        required property var modelData
                        readonly property bool on: root.card.multiple ? !!root.ticked[modelData.key] : modelData.chosen
                        width: Math.max(120, optLabel.implicitWidth + 36); height: 54
                        radius: 12
                        color: on ? theme.accent : theme.withAlpha(theme.accent, modelData.recommended ? 0.22 : 0.12)
                        border.width: modelData.recommended ? 1.6 : 0
                        border.color: theme.accent
                        ToolTip.visible: tileMouse.containsMouse && !!modelData.tip
                        ToolTip.text: modelData.tip ? modelData.tip.en : ""
                        Label {
                            id: optLabel
                            anchors.centerIn: parent
                            text: tile.modelData.label
                            color: tile.on ? theme.accentFg : theme.fg
                            font.pixelSize: 14; font.weight: Font.DemiBold
                        }
                        MouseArea {
                            id: tileMouse
                            anchors.fill: parent
                            hoverEnabled: true
                            cursorShape: Qt.PointingHandCursor
                            onClicked: {
                                if (root.card.multiple) {
                                    const t = Object.assign({}, root.ticked)
                                    t[tile.modelData.key] = !t[tile.modelData.key]
                                    root.ticked = t
                                } else {
                                    root.act({ do: "answer", card_id: root.card.id, choices: [tile.modelData.key] })
                                }
                            }
                        }
                    }
                }
                // several may be ticked: they go together
                Button {
                    visible: !!root.card.multiple
                    text: "Send"
                    enabled: Object.keys(root.ticked).some(k => k !== "__id" && root.ticked[k])
                    onClicked: root.act({ do: "answer", card_id: root.card.id, choices: Object.keys(root.ticked).filter(k => k !== "__id" && root.ticked[k]) })
                }
            }
            // something to read
            RowLayout {
                visible: root.answer.mode === "read"
                spacing: 10
                Button { text: "Got it"; onClicked: root.act({ do: "read", card_id: root.card.id }) }
            }
            // a request for approval
            RowLayout {
                visible: root.answer.mode === "request"
                spacing: 10
                Button { text: "Allow"; onClicked: root.act({ do: "verdict", request_id: root.card.id, allow: true }) }
                Button { text: "Deny"; onClicked: root.act({ do: "verdict", request_id: root.card.id, allow: false }) }
            }
            // what was said, or where it stands, and the way back
            Rectangle {
                visible: !!root.answer.label
                Layout.fillWidth: true
                implicitHeight: saidCol.implicitHeight + 24
                radius: 12
                color: theme.sunken
                ColumnLayout {
                    id: saidCol
                    anchors { left: parent.left; right: parent.right; verticalCenter: parent.verticalCenter; margins: 16 }
                    spacing: 4
                    Label { visible: !!root.answer.did; text: root.answer.did ? root.answer.did.en : ""; color: theme.faint; font.pixelSize: 12 }
                    Label { text: root.answer.label ? root.answer.label.en : ""; color: theme.fg; font.pixelSize: 18; font.weight: Font.Bold }
                    Label { visible: !!root.answer.detail; Layout.fillWidth: true; wrapMode: Text.Wrap; text: root.answer.detail ? root.answer.detail.en : ""; color: theme.muted; font.pixelSize: 13 }
                    Label { visible: !!root.answer.settled; Layout.fillWidth: true; wrapMode: Text.Wrap; text: root.answer.settled ? root.answer.settled.en : ""; color: theme.muted; font.pixelSize: 13 }
                    Label {
                        visible: root.answer.back === "reopen" || root.answer.back === "takeback"
                        text: "Decide again"
                        color: theme.accent; font.pixelSize: 13; font.weight: Font.DemiBold
                        MouseArea { anchors.fill: parent; cursorShape: Qt.PointingHandCursor; onClicked: root.act({ do: "decide_again", card_id: root.card.id }) }
                    }
                }
            }
            Label {
                visible: !!root.card.note && !!root.card.note.line
                Layout.fillWidth: true; wrapMode: Text.Wrap
                text: root.card.note && root.card.note.line ? root.card.note.line.en : ""
                color: theme.muted; font.pixelSize: 13
            }

            // the card's own buttons
            RowLayout {
                spacing: 18
                Layout.topMargin: 4
                Label {
                    text: root.card.star_says ? root.card.star_says.en : ""
                    color: root.card.starred ? theme.goldPen : theme.muted; font.pixelSize: 13; font.weight: Font.DemiBold
                    MouseArea { anchors.fill: parent; cursorShape: Qt.PointingHandCursor; onClicked: root.act({ do: "star", card_id: root.card.id, on: !root.card.starred }) }
                }
                Label {
                    visible: root.canAnswer
                    text: "Later"
                    color: theme.muted; font.pixelSize: 13; font.weight: Font.DemiBold
                    MouseArea { anchors.fill: parent; cursorShape: Qt.PointingHandCursor; onClicked: root.act({ do: "snooze", card_id: root.card.id }) }
                }
                Label {
                    visible: root.canAnswer
                    text: "What??"
                    color: theme.muted; font.pixelSize: 13; font.weight: Font.DemiBold
                    MouseArea { anchors.fill: parent; cursorShape: Qt.PointingHandCursor; onClicked: root.act({ do: "hand_back", card_id: root.card.id }) }
                }
            }

            // ---- the comments ------------------------------------------------------------------------------------
            Rectangle { Layout.fillWidth: true; Layout.topMargin: 14; height: 1; color: theme.line; visible: (root.card.talk || []).length > 0 }
            Repeater {
                model: root.card.talk || []
                delegate: ViewTalkRow {
                    required property var modelData
                    Layout.fillWidth: true
                    theme: root.theme
                    row: modelData
                    onOpen: (screen) => root.go(screen)
                }
            }
            TextField {
                Layout.fillWidth: true
                Layout.topMargin: 10
                visible: !!root.card.session_id
                placeholderText: root.card.ask ? root.card.ask.en : ""
                text: root.card.draft || ""
                onAccepted: {
                    if (text.trim() === "") return
                    root.act({ do: "write", session_id: root.card.session_id, card_id: root.card.id, text: text })
                    text = ""
                }
            }
        }
    }
}
