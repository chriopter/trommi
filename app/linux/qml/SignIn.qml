import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// Signing in on this machine: the account's e-mail and password, or the Emergency Kit. Either opens the room's
// recovery code, and this device joins the room with it.
Rectangle {
    id: root
    required property var trommi
    required property var theme
    color: theme.bg

    property bool kit: false

    Rectangle {
        anchors.centerIn: parent
        width: Math.min(440, parent.width - 48)
        height: form.implicitHeight + 64
        radius: 18
        color: theme.surface
        border.color: theme.line

        ColumnLayout {
            id: form
            anchors.fill: parent
            anchors.margins: 32
            spacing: 14

            Label {
                text: "Trommi"
                font.family: theme.display
                font.pixelSize: 30
                font.weight: Font.DemiBold
                color: theme.fg
            }
            Label {
                text: root.kit ? "Sign in with your Emergency Kit." : "Sign in to your room."
                color: theme.muted
                Layout.fillWidth: true
                wrapMode: Text.Wrap
            }

            // two ways in, as tiles
            RowLayout {
                Layout.fillWidth: true
                spacing: 8
                Repeater {
                    model: [{ k: false, t: "Password" }, { k: true, t: "Emergency Kit" }]
                    delegate: Rectangle {
                        required property var modelData
                        Layout.fillWidth: true
                        height: 36
                        radius: 10
                        color: root.kit === modelData.k ? theme.accentSoft : theme.sunken
                        border.color: root.kit === modelData.k ? theme.accent : "transparent"
                        Label {
                            anchors.centerIn: parent
                            text: modelData.t
                            color: root.kit === modelData.k ? theme.accent : theme.muted
                            font.weight: Font.Medium
                        }
                        MouseArea { anchors.fill: parent; cursorShape: Qt.PointingHandCursor; onClicked: root.kit = modelData.k }
                    }
                }
            }

            Label { text: "Hub"; color: theme.muted; font.pixelSize: 12 }
            TextField {
                id: hub
                background: Rectangle { implicitHeight: 40; radius: 10; color: theme.surface; border.color: parent.activeFocus ? theme.accent : theme.lineStrong }
                Layout.fillWidth: true
                text: trommi.hub_url
                placeholderText: "https://hub.trommi.com"
            }
            Label { text: root.kit ? "E-mail or account id" : "E-mail"; color: theme.muted; font.pixelSize: 12 }
            TextField {
                id: account
                background: Rectangle { implicitHeight: 40; radius: 10; color: theme.surface; border.color: parent.activeFocus ? theme.accent : theme.lineStrong }
                Layout.fillWidth: true
                placeholderText: root.kit ? "you@example.org" : "you@example.org"
                inputMethodHints: Qt.ImhEmailCharactersOnly | Qt.ImhNoAutoUppercase
            }
            Label { text: root.kit ? "The Emergency Kit's words" : "Password"; color: theme.muted; font.pixelSize: 12 }
            TextField {
                id: password
                background: Rectangle { implicitHeight: 40; radius: 10; color: theme.surface; border.color: parent.activeFocus ? theme.accent : theme.lineStrong }
                visible: !root.kit
                Layout.fillWidth: true
                echoMode: TextInput.Password
                onAccepted: go.clicked()
            }
            TextArea {
                id: words
                background: Rectangle { radius: 10; color: theme.surface; border.color: parent.activeFocus ? theme.accent : theme.lineStrong }
                visible: root.kit
                Layout.fillWidth: true
                Layout.preferredHeight: 84
                wrapMode: TextEdit.Wrap
                placeholderText: "the words, in order"
            }

            Label {
                visible: trommi.error !== ""
                text: trommi.error
                color: theme.critical
                wrapMode: Text.Wrap
                Layout.fillWidth: true
            }

            Button {
                id: go
                Layout.fillWidth: true
                Layout.topMargin: 6
                enabled: !trommi.busy && account.text.length > 0 && (root.kit ? words.text.length > 0 : password.text.length > 0)
                text: trommi.busy ? trommi.status : "Sign in"
                onClicked: {
                    if (root.kit) trommi.sign_in_kit(hub.text, account.text, words.text)
                    else trommi.sign_in_password(hub.text, account.text, password.text)
                }
                contentItem: Label {
                    text: go.text
                    color: go.enabled ? theme.accentFg : theme.faint
                    horizontalAlignment: Text.AlignHCenter
                    verticalAlignment: Text.AlignVCenter
                    font.weight: Font.DemiBold
                }
                background: Rectangle {
                    implicitHeight: 42
                    radius: 10
                    color: go.enabled ? theme.accent : theme.sunken
                }
            }
            Label {
                text: "Your password and your words never leave this device: they open your recovery code here, and this device joins your room with it."
                color: theme.faint
                font.pixelSize: 12
                wrapMode: Text.Wrap
                Layout.fillWidth: true
            }
        }
    }
}
