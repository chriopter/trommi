import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// The desktop sidebar: the Desk, then every session, each helper under its main session. A dot says whether the
// session's agent is online; a raised hand that it is stopped (the client core's red hand); a number how many of
// its questions wait.
Rectangle {
    id: root
    required property var trommi
    required property var theme
    required property var sessions
    required property int openCount
    color: theme.sunken

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: 12
        spacing: 4

        Label {
            text: "Trommi"
            font.family: theme.display
            font.pixelSize: 20
            font.weight: Font.DemiBold
            color: theme.fg
            Layout.leftMargin: 10
            Layout.topMargin: 6
            Layout.bottomMargin: 10
        }

        // the Desk
        Rectangle {
            Layout.fillWidth: true
            height: 38
            radius: 10
            color: trommi.selected === "" ? theme.surface : (deskHover.hovered ? Qt.darker(theme.sunken, 1.03) : "transparent")
            border.color: trommi.selected === "" ? theme.line : "transparent"
            HoverHandler { id: deskHover }
            RowLayout {
                anchors.fill: parent
                anchors.leftMargin: 12
                anchors.rightMargin: 12
                Label { text: "Desk"; color: theme.fg; font.weight: Font.DemiBold; Layout.fillWidth: true }
                Rectangle {
                    visible: root.openCount > 0
                    radius: 9
                    height: 18
                    width: Math.max(18, countText.implicitWidth + 10)
                    color: theme.accent
                    Label { id: countText; anchors.centerIn: parent; text: root.openCount; color: theme.accentFg; font.pixelSize: 11; font.weight: Font.DemiBold }
                }
            }
            MouseArea { anchors.fill: parent; cursorShape: Qt.PointingHandCursor; onClicked: trommi.select_session("") }
        }

        Label {
            text: "Sessions"
            color: theme.faint
            font.pixelSize: 11
            font.weight: Font.DemiBold
            font.capitalization: Font.AllUppercase
            font.letterSpacing: 0.6
            Layout.leftMargin: 12
            Layout.topMargin: 14
            Layout.bottomMargin: 2
        }

        ListView {
            Layout.fillWidth: true
            Layout.fillHeight: true
            clip: true
            spacing: 1
            model: root.sessions
            ScrollBar.vertical: ScrollBar { policy: ScrollBar.AsNeeded }
            delegate: Rectangle {
                id: row
                required property var modelData
                width: ListView.view.width
                height: 32
                radius: 8
                readonly property bool current: trommi.selected === modelData.id
                color: current ? theme.surface : (hover.hovered ? Qt.darker(theme.sunken, 1.03) : "transparent")
                border.color: current ? theme.line : "transparent"
                HoverHandler { id: hover }
                RowLayout {
                    anchors.fill: parent
                    anchors.leftMargin: 12 + row.modelData.depth * 16
                    anchors.rightMargin: 10
                    spacing: 8
                    Rectangle {
                        width: 7; height: 7; radius: 3.5
                        color: row.modelData.online ? theme.online : "transparent"
                        border.color: row.modelData.online ? theme.online : theme.faint
                        border.width: 1
                    }
                    Label {
                        text: row.modelData.name
                        color: row.modelData.depth > 0 ? theme.muted : theme.fg
                        font.weight: row.modelData.depth > 0 ? Font.Normal : Font.Medium
                        elide: Text.ElideRight
                        Layout.fillWidth: true
                    }
                    Label { visible: row.modelData.blocked; text: "✋"; font.pixelSize: 12 }
                    Label {
                        visible: row.modelData.waiting > 0 && !row.modelData.blocked
                        text: row.modelData.waiting
                        color: theme.muted
                        font.pixelSize: 11
                    }
                }
                MouseArea { anchors.fill: parent; cursorShape: Qt.PointingHandCursor; onClicked: trommi.select_session(row.modelData.id) }
            }
        }

        Label {
            text: trommi.title
            color: theme.faint
            font.pixelSize: 11
            Layout.leftMargin: 12
            Layout.bottomMargin: 4
        }
    }
}
