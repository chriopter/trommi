import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import "pen.mjs" as Pen

// The Desk, drawn from the app core's view model alone (ViewModel.desk): the big line the core says, Blitz with the
// core's words, the desk's goals, the sessions that are cut off, and what waits as rows in the order given. The
// only state of this file's own is whether the goals are folded.
Item {
    id: root
    required property var theme
    /** ViewModel.desk */
    required property var desk
    /** ViewModel.sidebar: a row's sender is drawn in its session's hue. */
    property var sidebar: ({})

    /** An `Action` (contract spelling). */
    signal act(var action)
    /** Another page: an `Event::Focus` screen. */
    signal go(var screen)

    /** A row's tile, as an action: the tile's shape and key say which; the card opens for the rest. */
    function tapped(row, key, shape) {
        if (shape === "got") act({ do: "read", card_id: row.id })
        else if (shape === "what") act({ do: "hand_back", card_id: row.id })
        else if (key !== "" && row.kind === "request") act({ do: "verdict", request_id: row.id, allow: key === "allow" })
        else if (key !== "") act({ do: "answer", card_id: row.id, choices: [key] })
        else go({ screen: "card", card_id: row.id })
    }

    property bool goalsFolded: true
    readonly property var rows: desk.rows || []
    readonly property var words: desk.hello ? desk.hello.en.split(" ") : []
    readonly property var goals: desk.goals || null
    readonly property var slugs: { const m = {}; for (const s of (sidebar.sessions || [])) m[s.id] = s.slug; return m }

    ListView {
        id: list
        anchors.fill: parent
        clip: true
        spacing: 16
        model: root.rows
        ScrollBar.vertical: ScrollBar { policy: ScrollBar.AsNeeded }
        header: Column {
            width: list.width
            topPadding: 56
            // (the head grows with the goals and the lines above the rows: the list stays at its top while it is there)
            onHeightChanged: if (!list.moving && list.contentY <= list.originY + height) Qt.callLater(list.positionViewAtBeginning)
            bottomPadding: 28
            spacing: 18
            Item {
                anchors.horizontalCenter: parent.horizontalCenter
                width: Math.min(800, list.width - 64); height: 60
                visible: root.words.length > 0
                Row {
                    id: hello
                    anchors.left: parent.left; anchors.leftMargin: 4
                    Label {
                        text: root.words.slice(0, -1).join(" ") + " "
                        visible: root.words.length > 1
                        font.family: theme.display; font.pixelSize: 46; font.weight: Font.ExtraBold; font.letterSpacing: -1.3
                        color: theme.fg
                    }
                    Label {
                        text: root.words.length ? root.words[root.words.length - 1] : ""
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
                // Blitz, while the core says there is something to walk through
                Rectangle {
                    visible: !!root.desk.blitz
                    anchors { right: parent.right; verticalCenter: hello.verticalCenter }
                    width: blitzRow.implicitWidth + 22; height: 41
                    radius: 10
                    rotation: -0.6
                    color: theme.surface
                    border.color: theme.fg; border.width: 1.6
                    Accessible.name: root.desk.blitz ? root.desk.blitz.en : ""
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
                            Label { id: n; anchors.centerIn: parent; text: root.desk.blitz && root.desk.blitz.args ? root.desk.blitz.args.n : ""; color: theme.bg; font.pixelSize: 12; font.weight: Font.Bold }
                        }
                    }
                }
            }
            // how far the reading of the room's past is
            Label {
                visible: !!root.desk.reading
                anchors.horizontalCenter: parent.horizontalCenter
                width: Math.min(800, list.width - 64)
                text: root.desk.reading ? root.desk.reading.en : ""
                color: theme.muted; font.pixelSize: 14
            }
            // the desk's goals: every line, or the first while folded
            Column {
                visible: !!root.goals && root.goals.lines.length > 0
                anchors.horizontalCenter: parent.horizontalCenter
                width: Math.min(800, list.width - 64)
                spacing: 3
                Repeater {
                    model: !root.goals ? [] : (root.goals.foldable && root.goalsFolded ? root.goals.lines.slice(0, 1) : root.goals.lines)
                    delegate: Label {
                        required property string modelData
                        width: parent.width
                        text: modelData
                        color: theme.muted; font.pixelSize: 15
                        wrapMode: Text.Wrap
                    }
                }
                Label {
                    visible: !!root.goals && root.goals.foldable
                    text: !root.goals ? "" : root.goalsFolded ? (root.goals.show_all ? root.goals.show_all.en : "") : "Show less"
                    color: theme.accent; font.pixelSize: 13; font.weight: Font.DemiBold
                    MouseArea { anchors.fill: parent; cursorShape: Qt.PointingHandCursor; onClicked: root.goalsFolded = !root.goalsFolded }
                }
            }
            // the sessions that are cut off, each with the step in its terminal
            Repeater {
                model: root.desk.slip || []
                delegate: Rectangle {
                    required property var modelData
                    anchors.horizontalCenter: parent.horizontalCenter
                    width: Math.min(800, list.width - 64)
                    height: slipCol.implicitHeight + 20
                    radius: 8
                    color: "transparent"
                    border.color: theme.lineStrong
                    Column {
                        id: slipCol
                        anchors { left: parent.left; right: parent.right; verticalCenter: parent.verticalCenter; leftMargin: 14; rightMargin: 14 }
                        spacing: 4
                        Label { width: parent.width; text: modelData.text.en; color: theme.muted; font.pixelSize: 13; wrapMode: Text.Wrap }
                        Row {
                            spacing: 8
                            Label { text: modelData.fix.say.en; color: theme.muted; font.pixelSize: 13 }
                            Label { visible: modelData.fix.code !== ""; text: modelData.fix.code; color: theme.fg; font.family: theme.mono; font.pixelSize: 13 }
                        }
                    }
                }
            }
        }
        footer: Item { height: 40 }
        delegate: Item {
            required property var modelData
            width: list.width
            height: 108
            MouseArea {
                anchors.fill: rowItem
                cursorShape: Qt.PointingHandCursor
                onClicked: root.go({ screen: "card", card_id: parent.modelData.id })
            }
            ViewDeskRow {
                id: rowItem
                anchors.horizontalCenter: parent.horizontalCenter
                width: Math.min(800, list.width - 64)
                theme: root.theme
                row: parent.modelData
                slug: parent.modelData.from ? (root.slugs[parent.modelData.from.session_id] || "") : ""
                onChosen: (key, shape) => root.tapped(row, key, shape)
            }
        }
    }

    // what stands in the list's place when nothing is open
    Label {
        anchors.centerIn: parent
        visible: !!root.desk.empty
        text: root.desk.empty ? root.desk.empty.en : ""
        color: theme.muted; font.pixelSize: 16
    }
}
