import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import QtQuick.Dialogs
import "pen.mjs" as Pen

// A session's Chat as the web shows it (session.css): its drawing and name at the top with the online dot, the
// agent's lines as plain text under its name, the person's own in a bubble at the right with the time under it, the
// session's link as a note when it does not hear, and the field to write in.
Item {
    id: root
    required property var trommi
    required property var theme
    required property var items
    required property var session
    /** Where fetched files lie (attachment id to file URL). */
    property var files: ({})
    /** Files of this machine that go with the next line (file URLs). */
    property var attach: []
    onSessionChanged: attach = []
    FileDialog {
        id: dialog
        title: "Files for " + (root.session ? root.session.name : "the session")
        fileMode: FileDialog.OpenFiles
        onAccepted: root.attach = root.attach.concat(selectedFiles.map(u => u.toString()))
    }
    function sendNow() {
        if (field.text.trim().length === 0 && root.attach.length === 0) return
        if (root.attach.length) root.trommi.send_with_files(field.text, JSON.stringify(root.attach))
        else root.trommi.send(field.text)
        field.text = ""
        root.attach = []
    }

    readonly property real hue: session ? Pen.hueFor({ id: session.board_id || session.id, mark: session.mark }) : 162
    readonly property bool gone: !!session && (session.link === "gone" || session.link === "cut")
    function clock(ms) {
        if (!ms) return ""
        const d = new Date(ms)
        return ("0" + d.getHours()).slice(-2) + ":" + ("0" + d.getMinutes()).slice(-2)
    }

    // ---- the head ----
    RowLayout {
        id: head
        anchors { top: parent.top; topMargin: 18; horizontalCenter: parent.horizontalCenter }
        width: Math.min(712, root.width - 64)
        height: 40
        spacing: 12
        Item {
            Layout.preferredWidth: 34; Layout.preferredHeight: 34
            PenMark {
                anchors.fill: parent
                svg: root.session ? Pen.doodleSvg(root.session.mark || root.session.board_id || root.session.id) : ""
                ink: root.session && root.session.online ? theme.tone(root.hue, "pen") : theme.grey(theme.tone(root.hue, "pen"))
                opacity: root.session && root.session.online ? 1 : 0.6
                lineWidth: 2
            }
            PenMark {
                visible: !!root.session && !!root.session.crown
                x: 2; y: -9; width: 22; height: 16
                svg: Pen.crownSvg(); turned: false
                ink: theme.goldPen; lineWidth: 1.7
                fills: ({ "crown-wash": theme.crownWash }); strokes: ({ "crown-wash": "none" })
            }
        }
        Label {
            text: root.session ? root.session.name : ""
            font.family: theme.display; font.pixelSize: 24; font.weight: Font.ExtraBold; font.letterSpacing: -0.5
            color: theme.fg
            elide: Text.ElideRight
            Layout.maximumWidth: head.width - 140
        }
        Rectangle {
            width: 12; height: 12; radius: 6
            color: root.session && root.session.online ? theme.online : theme.faint
            border.width: 2; border.color: theme.withAlpha(root.session && root.session.online ? theme.online : theme.faint, 0.35)
        }
        Item { Layout.fillWidth: true }
    }

    // ---- the lines ----
    ListView {
        id: list
        anchors { top: head.bottom; bottom: note.top; left: parent.left; right: parent.right; topMargin: 10; bottomMargin: 10 }
        clip: true
        spacing: 6
        model: root.items
        ScrollBar.vertical: ScrollBar { policy: ScrollBar.AsNeeded }
        onCountChanged: Qt.callLater(() => list.positionViewAtEnd())
        Component.onCompleted: Qt.callLater(() => list.positionViewAtEnd())
        delegate: Item {
            id: line
            required property var modelData
            required property int index
            readonly property bool mine: modelData.mine || modelData.human
            readonly property bool first: index === 0 || root.items[index - 1].mine !== modelData.mine || root.items[index - 1].human !== modelData.human
            width: list.width
            height: col.implicitHeight + (first ? 14 : 4)
            ColumnLayout {
                id: col
                anchors { bottom: parent.bottom; horizontalCenter: parent.horizontalCenter }
                width: Math.min(712, list.width - 64)
                spacing: 4
                // who writes, once per run of the agent's lines
                RowLayout {
                    visible: !line.mine && line.first
                    spacing: 8
                    PenMark {
                        Layout.preferredWidth: 20; Layout.preferredHeight: 20
                        svg: root.session ? Pen.doodleSvg(root.session.mark || root.session.board_id || root.session.id) : ""
                        ink: theme.tone(root.hue, "pen"); lineWidth: 2.2
                    }
                    Label { text: line.modelData.who; font.pixelSize: 13; font.weight: Font.Bold; color: theme.fg }
                    Label { text: root.clock(line.modelData.at); font.pixelSize: 12; color: theme.faint }
                }
                Text {
                    visible: !line.mine && !!line.modelData.text
                    Layout.fillWidth: true
                    text: line.modelData.text || ""
                    textFormat: Text.MarkdownText
                    font.family: theme.sans; font.pixelSize: 16; lineHeight: 1.45
                    color: theme.fg
                    wrapMode: Text.Wrap
                    onLinkActivated: (link) => Qt.openUrlExternally(link)
                }
                // the person's own line: a bubble at the right, the time under it
                Rectangle {
                    visible: line.mine && !!line.modelData.text
                    Layout.alignment: Qt.AlignRight
                    Layout.maximumWidth: col.width * 0.8
                    implicitWidth: Math.min(col.width * 0.8, mineText.implicitWidth + 32)
                    implicitHeight: mineText.implicitHeight + 22
                    radius: 18
                    color: theme.dark ? theme.surface2 : theme.sunken
                    opacity: line.modelData.pending ? 0.6 : 1
                    Text {
                        id: mineText
                        anchors { left: parent.left; right: parent.right; verticalCenter: parent.verticalCenter; leftMargin: 16; rightMargin: 16 }
                        text: line.modelData.text || ""
                        textFormat: Text.MarkdownText
                        font.family: theme.sans; font.pixelSize: 16
                        color: theme.fg
                        wrapMode: Text.Wrap
                    }
                }
                // the line's pictures and files
                Flow {
                    visible: (line.modelData.files || []).length > 0
                    Layout.fillWidth: true
                    layoutDirection: line.mine ? Qt.RightToLeft : Qt.LeftToRight
                    spacing: 8
                    Repeater {
                        model: line.modelData.files || []
                        delegate: Attachment {
                            required property var modelData
                            trommi: root.trommi; theme: root.theme
                            file: modelData; files: root.files
                            maxWidth: Math.min(420, col.width * 0.8); maxHeight: 300
                        }
                    }
                }
                Label {
                    visible: line.mine
                    Layout.alignment: Qt.AlignRight
                    Layout.rightMargin: 4
                    text: line.modelData.pending ? "sending …" : root.clock(line.modelData.at)
                    font.pixelSize: 12; color: theme.faint
                }
            }
        }
    }

    // ---- the session's link, when it does not hear ----
    Rectangle {
        id: note
        anchors { bottom: root.attach.length ? pending.top : composer.top; bottomMargin: 10; horizontalCenter: parent.horizontalCenter }
        width: Math.min(712, root.width - 64)
        visible: root.gone
        height: visible ? noteCol.implicitHeight + 24 : 0
        radius: 8
        color: theme.surface
        border.color: theme.line
        RowLayout {
            id: noteCol
            anchors { fill: parent; margins: 12 }
            spacing: 10
            PenMark { Layout.alignment: Qt.AlignTop; Layout.preferredWidth: 20; Layout.preferredHeight: 20; svg: Pen.sketchSvg(root.session && root.session.link === "cut" ? "ear-off" : "plug"); ink: theme.muted; lineWidth: 1.8 }
            ColumnLayout {
                Layout.fillWidth: true
                spacing: 6
                Text {
                    id: noteText
                    Layout.fillWidth: true
                    text: root.session ? (root.session.link === "cut"
                        ? root.session.name + " is cut off: it cannot hear you and cannot write to you."
                        : root.session.name + " is gone: it stopped without a word: its session ended, was killed, or its machine is off the network.") : ""
                    font.family: theme.sans; font.pixelSize: 13; color: theme.muted
                    wrapMode: Text.Wrap
                }
                // the way back, as a line to type (app.mjs linkOf: fix)
                RowLayout {
                    visible: !!root.session && root.session.link === "gone"
                    spacing: 6
                    Label { text: "Start it again in its folder:"; font.pixelSize: 13; color: theme.muted }
                    Rectangle {
                        implicitWidth: fix.implicitWidth + 12; implicitHeight: fix.implicitHeight + 6; radius: 4
                        color: theme.sunken
                        Label { id: fix; anchors.centerIn: parent; text: "claude --continue"; font.family: theme.mono; font.pixelSize: 13; color: theme.fg }
                    }
                }
            }
        }
    }

    // ---- files that go with the next line ----
    Flow {
        id: pending
        anchors { bottom: composer.top; bottomMargin: 8; horizontalCenter: parent.horizontalCenter }
        width: Math.min(712, root.width - 64)
        spacing: 8
        z: 2
        Repeater {
            model: root.attach
            delegate: Rectangle {
                required property string modelData
                required property int index
                height: 32; radius: 8
                width: att.implicitWidth + 40
                color: theme.sunken
                Label { id: att; anchors.verticalCenter: parent.verticalCenter; x: 12; text: decodeURIComponent(modelData.split("/").pop()); color: theme.fg; font.pixelSize: 13 }
                Label {
                    anchors { right: parent.right; rightMargin: 10; verticalCenter: parent.verticalCenter }
                    text: "×"; color: theme.muted; font.pixelSize: 16
                    MouseArea { anchors.fill: parent; anchors.margins: -6; cursorShape: Qt.PointingHandCursor; onClicked: { const a = root.attach.slice(); a.splice(index, 1); root.attach = a } }
                }
            }
        }
    }

    // ---- writing to the agent ----
    Rectangle {
        id: composer
        anchors { bottom: parent.bottom; bottomMargin: 16; horizontalCenter: parent.horizontalCenter }
        width: Math.min(712, root.width - 64)
        height: Math.min(160, Math.max(52, field.implicitHeight + 16))
        radius: 12
        color: theme.surface
        border.width: field.activeFocus ? 1.5 : 1
        border.color: field.activeFocus ? theme.fg : theme.lineStrong
        ScrollView {
            anchors { left: parent.left; right: clip.left; top: parent.top; bottom: parent.bottom; leftMargin: 6; rightMargin: 6 }
            TextArea {
                id: field
                placeholderText: root.gone ? "It is gone. What you write waits for it" : (root.session ? "Write to " + root.session.name : "Write …")
                placeholderTextColor: theme.faint
                color: theme.fg
                font.family: theme.sans; font.pixelSize: 16
                wrapMode: TextEdit.Wrap
                background: null
                verticalAlignment: TextEdit.AlignVCenter
                Keys.onPressed: (e) => {
                    if ((e.key === Qt.Key_Return || e.key === Qt.Key_Enter) && !(e.modifiers & Qt.ShiftModifier)) {
                        root.sendNow()
                        e.accepted = true
                    }
                }
            }
        }
        PenMark {
            id: clip
            anchors { right: send.left; rightMargin: 10; verticalCenter: parent.verticalCenter }
            width: 20; height: 20
            svg: Pen.sketchSvg("clip"); ink: theme.muted; lineWidth: 1.8
            MouseArea { anchors.fill: parent; anchors.margins: -8; cursorShape: Qt.PointingHandCursor; onClicked: dialog.open() }
        }
        Rectangle {
            id: send
            anchors { right: parent.right; rightMargin: 6; verticalCenter: parent.verticalCenter }
            width: 38; height: 38; radius: 10
            color: field.text.trim().length > 0 || root.attach.length > 0 ? theme.accent : theme.withAlpha(theme.accent, 0.4)
            PenMark { anchors.centerIn: parent; width: 22; height: 22; svg: Pen.sketchSvg("send"); ink: theme.accentFg; lineWidth: 2.2 }
            MouseArea {
                id: sendArea
                anchors.fill: parent
                cursorShape: Qt.PointingHandCursor
                onClicked: root.sendNow()
            }
        }
    }
}
