import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// The Desk: the open cards in the client core's order (what blocks first, then what knocks, then the rest), then
// the open permission requests.
Item {
    id: root
    required property var trommi
    required property var theme
    required property var cards

    ListView {
        id: list
        anchors.fill: parent
        anchors.topMargin: 8
        clip: true
        spacing: 14
        model: root.cards
        header: Item { height: 10 }
        footer: Item { height: 28 }
        ScrollBar.vertical: ScrollBar { policy: ScrollBar.AsNeeded }
        delegate: Item {
            required property var modelData
            width: list.width
            height: card.height
            CardView {
                id: card
                anchors.horizontalCenter: parent.horizontalCenter
                width: Math.min(760, list.width - 56)
                trommi: root.trommi
                theme: root.theme
                card: parent.modelData
            }
        }
    }

    Column {
        anchors.centerIn: parent
        visible: root.cards.length === 0
        spacing: 6
        Label { text: "Nothing waits for you."; color: theme.fg; font.pixelSize: 18; anchors.horizontalCenter: parent.horizontalCenter }
        Label { text: "New questions from your agents land here."; color: theme.muted; anchors.horizontalCenter: parent.horizontalCenter }
    }
}
