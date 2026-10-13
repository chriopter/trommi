import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import "pen.mjs" as Pen

// The Desk as the web shows it (desk.css, desk.mjs deskHead): one big line in the display face, its last word
// underlined with the pen, Blitz with the count at the right; under it the open questions in the client core's
// order, one row each, at most 800 wide.
Item {
    id: root
    required property var trommi
    required property var theme
    required property var cards
    required property var sessions
    property var files: ({})
    signal openCard(string id)

    // one of the web's greetings, picked once per start (desk.mjs GREETINGS); a calm one when nothing waits
    readonly property var greetings: ['Welcome back.', 'There you are.', 'The agents missed you.', 'Desk’s all yours.', 'Ring the bell.', 'Decisions, decisions.',
        'Your call.', 'Back at it.', 'Somebody knocked.', 'Look who’s here.', 'They’ve been waiting.', 'The floor is yours.',
        'Pick a card.', 'Over to you.', 'Right on time.', 'Pull up a chair.', 'Yes or no?', 'The boss is in.', 'What’ll it be?', 'Ready when you are.']
    readonly property var calm: ['All quiet.', 'Nothing needs you.', 'Clear desk.', 'Carry on.', 'As you were.', 'Go outside.']
    property real dice: Math.random()
    readonly property string greeting: {
        const set = cards.length ? greetings : calm
        // (TROMMI_LINUX_GREETING: one line for pictures beside the web's)
        return trommi.greeting !== "" ? trommi.greeting : set[Math.floor(dice * set.length)]
    }
    readonly property var words: greeting.split(" ")

    function sessionOf(id) {
        for (const s of sessions) if (s.id === id) return s
        return null
    }

    ListView {
        id: list
        anchors.fill: parent
        clip: true
        spacing: 16
        model: root.cards
        ScrollBar.vertical: ScrollBar { policy: ScrollBar.AsNeeded }
        header: Item {
            width: list.width
            height: 154
            Item {
                anchors { horizontalCenter: parent.horizontalCenter; top: parent.top; topMargin: 56 }
                width: Math.min(800, list.width - 64); height: 60
                Row {
                    id: hello
                    anchors.left: parent.left; anchors.leftMargin: 4
                    spacing: 0
                    Label {
                        text: root.words.slice(0, -1).join(" ") + " "
                        visible: text !== ""
                        font.family: theme.display; font.pixelSize: 46; font.weight: Font.ExtraBold; font.letterSpacing: -1.3
                        color: theme.fg
                    }
                    Label {
                        id: last
                        text: root.words[root.words.length - 1]
                        font.family: theme.display; font.pixelSize: 46; font.weight: Font.ExtraBold; font.letterSpacing: -1.3
                        color: theme.fg
                        PenMark {
                            anchors { left: parent.left; right: parent.right; top: parent.bottom; topMargin: -6; rightMargin: 10 }
                            height: 9; turned: false
                            svg: '<svg viewBox="0 0 100 9"><path d="M1 5.6 Q22 3.2 48 4.4 T99 3.6"/></svg>'
                            ink: theme.accent; lineWidth: 2.4
                        }
                    }
                }
                // Blitz: the count of what waits, card by card
                Rectangle {
                    visible: root.cards.length > 0
                    anchors { right: parent.right; verticalCenter: hello.verticalCenter }
                    width: blitzRow.implicitWidth + 22; height: 41
                    radius: 10
                    rotation: -0.6
                    color: theme.surface
                    border.color: theme.fg; border.width: 1.6
                    Rectangle { z: -1; x: 2; y: 3; width: parent.width; height: parent.height; radius: 10; color: theme.withAlpha(theme.fg, 0.85) }
                    Row {
                        id: blitzRow
                        anchors.centerIn: parent
                        spacing: 8
                        PenMark { width: 20; height: 20; anchors.verticalCenter: parent.verticalCenter; svg: Pen.sketchSvg("spark"); ink: theme.fg; lineWidth: 2 }
                        Label { anchors.verticalCenter: parent.verticalCenter; text: "Blitz"; font.pixelSize: 13; font.weight: Font.Bold; color: theme.fg }
                        Rectangle {
                            anchors.verticalCenter: parent.verticalCenter
                            width: Math.max(26, n.implicitWidth + 12); height: 26; radius: 13
                            color: theme.fg
                            Label { id: n; anchors.centerIn: parent; text: root.cards.length; color: theme.bg; font.pixelSize: 12; font.weight: Font.Bold }
                        }
                    }
                    MouseArea { anchors.fill: parent; cursorShape: Qt.PointingHandCursor; onClicked: if (root.cards.length) root.openCard(root.cards[0].id) }
                }
            }
        }
        footer: Item { height: 40 }
        delegate: Item {
            required property var modelData
            width: list.width
            height: 108
            DeskRow {
                anchors.horizontalCenter: parent.horizontalCenter
                width: Math.min(800, list.width - 64)
                trommi: root.trommi
                theme: root.theme
                card: parent.modelData
                from: root.sessionOf(parent.modelData.session_id)
                files: root.files
                onOpen: root.openCard(parent.modelData.id)
            }
        }
    }

    Column {
        anchors.centerIn: parent
        visible: root.cards.length === 0
        spacing: 8
        Label { text: "New questions from your agents land here."; color: theme.muted; font.pixelSize: 16; anchors.horizontalCenter: parent.horizontalCenter }
    }
}
