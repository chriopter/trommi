import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// A session's Chat: the agent's lines on the left, the person's on the right, and a field to write to the agent.
Item {
    id: root
    required property var trommi
    required property var theme
    required property var items
    required property var session

    ColumnLayout {
        anchors.fill: parent
        spacing: 0

        ListView {
            id: list
            Layout.fillWidth: true
            Layout.fillHeight: true
            clip: true
            spacing: 10
            model: root.items
            header: Item { height: 18 }
            footer: Item { height: 18 }
            ScrollBar.vertical: ScrollBar { policy: ScrollBar.AsNeeded }
            onCountChanged: Qt.callLater(() => list.positionViewAtEnd())
            delegate: Item {
                id: line
                required property var modelData
                width: list.width
                height: bubble.height
                Rectangle {
                    id: bubble
                    readonly property real most: Math.min(620, line.width * 0.72)
                    width: Math.min(most, Math.max(who.implicitWidth, text.implicitWidth) + 28)
                    height: col.implicitHeight + 20
                    x: line.modelData.mine || line.modelData.human ? line.width - width - 32 : 32
                    radius: 14
                    color: line.modelData.mine || line.modelData.human ? theme.accentSoft : theme.surface
                    border.color: line.modelData.mine || line.modelData.human ? "transparent" : theme.line
                    opacity: line.modelData.pending ? 0.6 : 1
                    ColumnLayout {
                        id: col
                        anchors.left: parent.left
                        anchors.right: parent.right
                        anchors.top: parent.top
                        anchors.margins: 10
                        anchors.leftMargin: 14
                        anchors.rightMargin: 14
                        spacing: 3
                        Label {
                            id: who
                            text: line.modelData.who + (line.modelData.ago ? " · " + line.modelData.ago : "")
                            color: theme.faint
                            font.pixelSize: 11
                        }
                        Text {
                            id: text
                            text: line.modelData.text || ""
                            textFormat: Text.MarkdownText
                            color: theme.fg
                            font.family: theme.sans
                            font.pixelSize: 14
                            wrapMode: Text.Wrap
                            Layout.fillWidth: true
                            Layout.maximumWidth: bubble.most - 28
                            onLinkActivated: (link) => Qt.openUrlExternally(link)
                        }
                    }
                }
            }
        }

        Label {
            visible: root.items.length === 0
            text: "No messages in this session yet."
            color: theme.muted
            Layout.alignment: Qt.AlignHCenter
            Layout.bottomMargin: 30
        }

        // writing to the agent
        Rectangle {
            Layout.fillWidth: true
            Layout.preferredHeight: Math.min(160, field.implicitHeight + 28)
            color: theme.bg
            Rectangle { width: parent.width; height: 1; color: theme.line }
            RowLayout {
                anchors.fill: parent
                anchors.margins: 12
                anchors.leftMargin: 28
                anchors.rightMargin: 28
                spacing: 10
                ScrollView {
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    TextArea {
                        id: field
                        placeholderText: root.session ? "Write to " + root.session.name : "Write …"
                        wrapMode: TextEdit.Wrap
                        background: Rectangle { radius: 10; color: theme.surface; border.color: theme.lineStrong }
                        Keys.onPressed: (e) => {
                            if ((e.key === Qt.Key_Return || e.key === Qt.Key_Enter) && !(e.modifiers & Qt.ShiftModifier)) {
                                send.clicked()
                                e.accepted = true
                            }
                        }
                    }
                }
                Button {
                    id: send
                    text: "Send"
                    enabled: field.text.trim().length > 0
                    contentItem: Label { text: send.text; color: send.enabled ? theme.accentFg : theme.faint; font.weight: Font.DemiBold; horizontalAlignment: Text.AlignHCenter; verticalAlignment: Text.AlignVCenter }
                    background: Rectangle { implicitWidth: 84; implicitHeight: 40; radius: 10; color: send.enabled ? theme.accent : theme.sunken }
                    onClicked: {
                        trommi.send(field.text)
                        field.text = ""
                    }
                }
            }
        }
    }
}
