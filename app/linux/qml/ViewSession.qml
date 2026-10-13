import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import "pen.mjs" as Pen

// One session's page, drawn from the app core's view model alone (ViewModel.sessions[id]): its heading, its
// conversation, the status lines of its running work, and the field to write in. What is typed goes out as
// `Action::Write`; what it means is the core's. The only state of this file's own is the text being typed.
Item {
    id: root
    required property var theme
    /** ViewModel.sessions[id] */
    required property var session
    /** An `Action` (contract spelling). */
    signal act(var action)
    /** Another page: an `Event::Focus` screen. */
    signal go(var screen)

    readonly property var dot: session.dot || ({ on: false, word: { en: "" } })
    readonly property real hue: Pen.hueFor({ id: session.slug || session.id || "", mark: session.mark || "" })

    function write(text) {
        if (text.trim() === "") return
        root.act({ do: "write", session_id: root.session.id, text: text })
    }

    ColumnLayout {
        anchors.fill: parent
        spacing: 0

        // ---- the heading -----------------------------------------------------------------------------------------
        Item {
            Layout.fillWidth: true
            Layout.preferredHeight: 88
            PenMark {
                id: headMark
                anchors { left: parent.left; leftMargin: 40; verticalCenter: parent.verticalCenter }
                width: 40; height: 40
                svg: Pen.doodleSvg(root.session.mark || root.session.slug || root.session.id || "")
                ink: root.session.online ? theme.tone(root.hue, "pen") : theme.grey(theme.tone(root.hue, "pen"))
                lineWidth: 2
            }
            Column {
                anchors { left: headMark.right; leftMargin: 12; right: openLabel.left; rightMargin: 12; verticalCenter: parent.verticalCenter }
                spacing: 2
                Label {
                    visible: (root.session.path || []).length > 1
                    text: (root.session.path || []).slice(0, -1).join(" › ")
                    color: theme.faint; font.pixelSize: 12
                }
                Row {
                    spacing: 8
                    Label {
                        id: nameLabel
                        text: root.session.name ? root.session.name.en : ""
                        font.family: theme.display; font.pixelSize: 26; font.weight: Font.ExtraBold
                        color: theme.fg
                    }
                    Rectangle {
                        anchors.verticalCenter: nameLabel.verticalCenter
                        width: 9; height: 9; radius: 4.5
                        color: root.dot.on ? theme.online : theme.faint
                        ToolTip.visible: dotMouse.containsMouse
                        ToolTip.text: root.dot.word ? root.dot.word.en : ""
                        Accessible.name: root.dot.says ? root.dot.says.en : ""
                        MouseArea { id: dotMouse; anchors.fill: parent; hoverEnabled: true }
                    }
                    Label {
                        anchors.verticalCenter: nameLabel.verticalCenter
                        visible: (root.session.model_label || "") !== ""
                        text: root.session.model_label || ""
                        color: theme.faint; font.pixelSize: 13
                    }
                }
                Label {
                    visible: !!root.session.quiet
                    text: root.session.quiet ? root.session.quiet.en : ""
                    color: theme.faint; font.pixelSize: 12
                }
            }
            // its questions that wait: the way to the first
            Label {
                id: openLabel
                anchors { right: parent.right; rightMargin: 40; verticalCenter: parent.verticalCenter }
                visible: !!root.session.open_word
                text: root.session.open_word ? root.session.open_word.en : ""
                color: theme.urgHigh; font.pixelSize: 13; font.weight: Font.Bold
                Accessible.name: root.session.open_says ? root.session.open_says.en : ""
                MouseArea {
                    anchors.fill: parent; cursorShape: Qt.PointingHandCursor
                    onClicked: {
                        const i = (root.session.talk || []).findIndex(r => r.id === root.session.first_open)
                        if (i >= 0) talk.positionViewAtIndex(i, ListView.Center)
                    }
                }
            }
            Rectangle { anchors.bottom: parent.bottom; width: parent.width; height: 1; color: theme.line }
        }

        // ---- the status lines of its running work ----------------------------------------------------------------
        Column {
            Layout.fillWidth: true
            visible: (root.session.status_lines || []).length > 0
            topPadding: 8; bottomPadding: 8
            Repeater {
                model: root.session.status_lines || []
                delegate: RowLayout {
                    required property var modelData
                    x: 40
                    width: root.width - 80
                    spacing: 10
                    Label { text: modelData.label; color: theme.fg; font.pixelSize: 13; font.weight: Font.DemiBold }
                    Label { text: modelData.detail; color: theme.muted; font.pixelSize: 13; Layout.fillWidth: true; elide: Text.ElideRight }
                    Label { text: modelData.says.en; color: theme.faint; font.pixelSize: 12 }
                }
            }
        }

        // ---- the conversation -----------------------------------------------------------------------------------
        ListView {
            id: talk
            Layout.fillWidth: true
            Layout.fillHeight: true
            clip: true
            model: root.session.talk || []
            spacing: 0
            leftMargin: 40; rightMargin: 40
            ScrollBar.vertical: ScrollBar { policy: ScrollBar.AsNeeded }
            onCountChanged: Qt.callLater(positionViewAtEnd)
            delegate: ViewTalkRow {
                required property var modelData
                width: talk.width - 80
                theme: root.theme
                row: modelData
                onOpen: (screen) => root.go(screen)
            }

            // an empty conversation: who joined, and three ways to start
            Column {
                visible: !!root.session.empty
                anchors.centerIn: parent
                width: Math.min(520, parent.width - 80)
                spacing: 12
                Label { visible: !!root.session.empty && root.session.empty.meta !== ""; text: root.session.empty ? root.session.empty.meta : ""; color: theme.faint; font.pixelSize: 12; anchors.horizontalCenter: parent.horizontalCenter }
                Label { width: parent.width; horizontalAlignment: Text.AlignHCenter; wrapMode: Text.Wrap; text: root.session.empty ? root.session.empty.text.en : ""; color: theme.muted; font.pixelSize: 15 }
                Repeater {
                    model: root.session.empty ? root.session.empty.starters : []
                    delegate: Rectangle {
                        required property var modelData
                        anchors.horizontalCenter: parent.horizontalCenter
                        width: starter.implicitWidth + 28; height: 34; radius: 17
                        color: theme.sunken
                        Label { id: starter; anchors.centerIn: parent; text: modelData.en; color: theme.fg; font.pixelSize: 13 }
                        MouseArea { anchors.fill: parent; cursorShape: Qt.PointingHandCursor; onClicked: root.write(modelData.en) }
                    }
                }
            }
        }

        // ---- above the field: why it does not hear, or his receipt; then the field --------------------------------
        Label {
            visible: !!root.session.answering
            Layout.leftMargin: 40
            text: root.session.answering ? root.session.answering.en : ""
            color: theme.faint; font.pixelSize: 12
        }
        ColumnLayout {
            visible: !!root.session.note
            Layout.leftMargin: 40; Layout.rightMargin: 40
            Layout.fillWidth: true
            spacing: 2
            readonly property var note: root.session.note || ({})
            Label { visible: !!parent.note.receipt; text: parent.note.receipt ? parent.note.receipt.en : ""; color: theme.faint; font.pixelSize: 12 }
            Label { visible: !!parent.note.line; Layout.fillWidth: true; wrapMode: Text.Wrap; text: parent.note.line ? parent.note.line.en : ""; color: theme.muted; font.pixelSize: 13 }
            RowLayout {
                visible: !!parent.note.fix
                spacing: 8
                Label { text: parent.parent.note.fix ? parent.parent.note.fix.say.en : ""; color: theme.muted; font.pixelSize: 13 }
                Label { text: parent.parent.note.fix ? parent.parent.note.fix.code : ""; color: theme.fg; font.family: theme.mono; font.pixelSize: 13 }
            }
        }
        TextField {
            id: field
            Layout.fillWidth: true
            Layout.margins: 24
            Layout.leftMargin: 40; Layout.rightMargin: 40
            placeholderText: root.session.compose ? root.session.compose.en : ""
            onAccepted: { root.write(text); text = "" }
        }
    }
}
