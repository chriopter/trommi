import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// Signing in on this machine, as the web's login (auth.css .ob-*): the bell and the name, one big line, the account
// and the password; "Forgot?" turns it into the Emergency Kit's words. Either opens the room's recovery code here, and
// this device joins the room with it.
Rectangle {
    id: root
    required property var trommi
    required property var theme
    color: theme.bg

    property bool kit: false
    readonly property string bell: '<svg viewBox="0 0 24 24"><path d="M3.1 19Q12.3 18.6 16.7 19L21 19.3"/><path d="M4.8 18.9Q5.2 15.1 5.7 13.7Q6.2 12.3 7.5 11.3Q8.7 10.3 10.3 9.5Q12 8.8 13.7 9.3Q15.3 9.8 16.4 11Q17.5 12.2 18.1 13.7Q18.6 15.2 18.8 17L18.9 18.8"/><path d="M11.8 8.6L12.2 6.9"/><path d="M10 6.6Q11.8 6 12.8 6.4L13.8 6.8"/><path d="M18.5 7.3Q19.5 5.8 19.8 5.2L20 4.5"/><path d="M20.6 10.3Q21.5 9.4 22.3 8.9L23.1 8.5"/></svg>'

    component Field: TextField {
        Layout.fillWidth: true
        implicitHeight: 48
        leftPadding: 15; rightPadding: 15
        font.family: theme.sans; font.pixelSize: 16
        color: theme.fg
        placeholderTextColor: theme.faint
        background: Rectangle {
            radius: 10
            color: theme.surface
            border.width: parent.activeFocus ? 2 : 1
            border.color: parent.activeFocus ? theme.fg : theme.lineStrong
        }
    }

    ColumnLayout {
        id: form
        anchors.horizontalCenter: parent.horizontalCenter
        y: Math.max(40, (parent.height - implicitHeight) / 2 - 30)
        width: Math.min(384, parent.width - 48)
        spacing: 6

        RowLayout {
            spacing: 9
            Layout.bottomMargin: 38
            PenMark { Layout.preferredWidth: 30; Layout.preferredHeight: 30; svg: root.bell; ink: theme.accent; lineWidth: 1.8; turned: false }
            Label { text: "Trommi"; font.family: theme.display; font.pixelSize: 21; font.weight: Font.ExtraBold; color: theme.fg }
        }
        Label {
            text: root.kit ? "Emergency Kit" : "Log in"
            font.family: theme.display; font.pixelSize: 44; font.weight: Font.ExtraBold; font.letterSpacing: -1.2
            color: theme.fg
            Layout.bottomMargin: 10
        }

        Label { text: "Email or account ID"; font.pixelSize: 14; font.weight: Font.DemiBold; color: theme.fg; Layout.topMargin: 10 }
        Field { id: account; placeholderText: "you@example.org"; inputMethodHints: Qt.ImhEmailCharactersOnly | Qt.ImhNoAutoUppercase }

        RowLayout {
            Layout.fillWidth: true
            Layout.topMargin: 10
            Label { text: root.kit ? "The Kit’s words" : "Password"; font.pixelSize: 14; font.weight: Font.DemiBold; color: theme.fg; Layout.fillWidth: true }
            Label {
                text: root.kit ? "Password" : "Forgot?"
                font.pixelSize: 14; font.weight: Font.DemiBold; font.underline: true
                color: theme.accent
                MouseArea { anchors.fill: parent; cursorShape: Qt.PointingHandCursor; onClicked: root.kit = !root.kit }
            }
        }
        Field { id: password; visible: !root.kit; echoMode: TextInput.Password; onAccepted: go.clicked() }
        TextArea {
            id: words
            visible: root.kit
            Layout.fillWidth: true
            Layout.preferredHeight: 96
            leftPadding: 15; rightPadding: 15; topPadding: 12
            wrapMode: TextEdit.Wrap
            font.family: theme.mono; font.pixelSize: 15
            color: theme.fg
            placeholderText: "the twelve words, in order"
            placeholderTextColor: theme.faint
            background: Rectangle { radius: 10; color: theme.surface; border.width: words.activeFocus ? 2 : 1; border.color: words.activeFocus ? theme.fg : theme.lineStrong }
        }

        Label {
            visible: trommi.error !== ""
            text: trommi.error
            color: theme.urgCritical
            font.pixelSize: 14; font.weight: Font.Medium
            wrapMode: Text.Wrap
            Layout.fillWidth: true
            Layout.topMargin: 4
        }

        Button {
            id: go
            Layout.fillWidth: true
            Layout.topMargin: 14
            enabled: !trommi.busy && account.text.length > 0 && (root.kit ? words.text.length > 0 : password.text.length > 0)
            text: trommi.busy ? trommi.status : "Log in"
            onClicked: {
                if (root.kit) trommi.sign_in_kit(hub.text, account.text, words.text)
                else trommi.sign_in_password(hub.text, account.text, password.text)
            }
            contentItem: Label {
                text: go.text
                color: theme.accentFg
                horizontalAlignment: Text.AlignHCenter
                verticalAlignment: Text.AlignVCenter
                font.pixelSize: 16; font.weight: Font.DemiBold
            }
            background: Rectangle {
                implicitHeight: 50
                radius: 10
                color: go.hovered && go.enabled ? theme.accentHover : theme.accent
                opacity: go.enabled || trommi.busy ? 1 : 0.55
            }
        }

        Label {
            Layout.fillWidth: true
            Layout.topMargin: 18
            text: "Your password and the Kit’s words never leave this device: they open your recovery code here, and this device joins your room with it."
            color: theme.muted
            font.pixelSize: 13
            wrapMode: Text.Wrap
            horizontalAlignment: Text.AlignHCenter
        }
    }

    // the hub, quiet at the foot (another one for self-hosting and local work)
    RowLayout {
        anchors { bottom: parent.bottom; horizontalCenter: parent.horizontalCenter; bottomMargin: 22 }
        spacing: 8
        Label { text: "Hub"; color: theme.faint; font.pixelSize: 13 }
        TextField {
            id: hub
            text: trommi.hub_url
            implicitWidth: 260
            font.pixelSize: 13
            color: theme.muted
            background: Rectangle { radius: 8; color: "transparent"; border.width: hub.activeFocus ? 1 : 0; border.color: theme.lineStrong }
        }
    }
}
