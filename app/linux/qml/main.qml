import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import QtQuick.Window
import com.trommi.linux

ApplicationWindow {
    id: window
    width: 1280
    height: 820
    minimumWidth: 760
    minimumHeight: 520
    visible: true
    title: "Trommi"
    color: theme.bg

    // The web board's tokens (app/web/public/app.css), light and dark.
    QtObject {
        id: theme
        // (Qt.styleHints.colorScheme: Qt 6.5 and later; 2 is Dark. Older Qt: light)
        readonly property bool dark: Qt.styleHints.colorScheme === 2
        readonly property color bg: dark ? "#0e1311" : "#f5f6f2"
        readonly property color surface: dark ? "#171d1a" : "#ffffff"
        readonly property color sunken: dark ? "#111715" : "#eceee8"
        readonly property color fg: dark ? "#e9eeea" : "#141c18"
        readonly property color muted: dark ? "#9aa8a0" : "#5c6862"
        readonly property color faint: dark ? "#6c7a73" : "#8a958f"
        readonly property color line: dark ? "#252f2a" : "#e1e5df"
        readonly property color lineStrong: dark ? "#35423b" : "#c9d0c8"
        readonly property color accent: dark ? "#6fd0b5" : "#1b6a57"
        readonly property color accentSoft: dark ? "#17332b" : "#dcefe8"
        readonly property color accentFg: dark ? "#08130f" : "#ffffff"
        readonly property color high: dark ? "#f2a56c" : "#b4551b"
        readonly property color highSoft: dark ? "#3a2415" : "#fbe9dc"
        readonly property color critical: dark ? "#ff8a80" : "#b3261e"
        readonly property color criticalSoft: dark ? "#41191a" : "#fbe0de"
        readonly property color working: "#b07a06"
        readonly property color online: dark ? "#6fd0b5" : "#1f8a4c"
        readonly property string sans: Qt.fontFamilies().indexOf("IBM Plex Sans") >= 0 ? "IBM Plex Sans"
            : (Qt.fontFamilies().indexOf("Adwaita Sans") >= 0 ? "Adwaita Sans" : Application.font.family)
        readonly property string display: Qt.fontFamilies().indexOf("Bricolage Grotesque") >= 0 ? "Bricolage Grotesque" : sans
    }

    font.family: theme.sans
    font.pixelSize: 14
    // the controls (Qt Quick's Basic style) in the board's colours
    palette.window: theme.bg
    palette.base: theme.surface
    palette.alternateBase: theme.sunken
    palette.text: theme.fg
    palette.windowText: theme.fg
    palette.button: theme.sunken
    palette.buttonText: theme.fg
    palette.highlight: theme.accent
    palette.highlightedText: theme.accentFg
    palette.mid: theme.lineStrong
    palette.dark: theme.lineStrong
    palette.midlight: theme.line
    palette.light: theme.surface
    palette.placeholderText: theme.faint

    Trommi {
        id: trommi
        Component.onCompleted: trommi.start()
    }

    property var desk: { try { return JSON.parse(trommi.desk_json) } catch (e) { return [] } }
    property var sessions: { try { return JSON.parse(trommi.sessions_json) } catch (e) { return [] } }
    property var chat: { try { return JSON.parse(trommi.chat_json) } catch (e) { return [] } }
    property var selectedSession: {
        for (let i = 0; i < sessions.length; i++) if (sessions[i].id === trommi.selected) return sessions[i]
        return null
    }

    // ---- a scene for pictures and the smoke test: TROMMI_LINUX_SHOT=<file.png> [TROMMI_LINUX_SCENE=chat]
    Timer {
        id: shotTimer
        interval: Math.max(200, trommi.shot_after_ms)
        running: trommi.shot_path !== "" && (trommi.signed_in ? window.desk.length > 0 || trommi.scene === "empty" : !trommi.starting)
        // a scene's steps, one per tick: `chat` opens a session (TROMMI_LINUX_SESSION names it), `say` also writes
        // to its agent, `answer` answers the first decision card with its first option; then the picture
        property int step: 0
        onTriggered: {
            const scene = trommi.scene
            if ((scene === "chat" || scene === "say") && step === 0) {
                step = 1
                let pick = ""
                for (let i = 0; i < window.sessions.length; i++) {
                    const s = window.sessions[i]
                    if (s.name === trommi.scene_session || (pick === "" && trommi.scene_session === "" && s.depth === 0 && s.online)) pick = s.id
                }
                if (pick === "" && window.sessions.length > 0) pick = window.sessions[0].id
                trommi.select_session(pick)
                shotTimer.restart()
                return
            }
            if (scene === "say" && step === 1) {
                step = 2
                trommi.send("Looks good from the Linux app. Ship the split hero, please.")
                shotTimer.restart()
                return
            }
            if (scene === "answer" && step === 0) {
                step = 1
                for (let i = 0; i < window.desk.length; i++) {
                    const c = window.desk[i]
                    if (c.kind === "card" && c.card_type !== "info" && !c.multiple && c.options.length > 0) { trommi.answer(c.id, c.options[0].key); break }
                }
                shotTimer.restart()
                return
            }
            page.grabToImage(function (shot) {
                shot.saveToFile(trommi.shot_path)
                trommi.shot_taken()
                Qt.quit()
            })
        }
    }

    // everything the window shows, in one item (a picture of it is a picture of the window)
    Rectangle {
        id: page
        anchors.fill: parent
        color: theme.bg

        // ---- signing in --------------------------------------------------------------------------------------------
        SignIn {
            anchors.fill: parent
            visible: !trommi.signed_in && !trommi.starting
            trommi: trommi
            theme: theme
        }

        // ---- starting --------------------------------------------------------------------------------------------
        Item {
            anchors.fill: parent
            visible: trommi.starting
            Column {
                anchors.centerIn: parent
                spacing: 12
                BusyIndicator { anchors.horizontalCenter: parent.horizontalCenter; running: trommi.starting }
                Label { text: trommi.status; color: theme.muted; anchors.horizontalCenter: parent.horizontalCenter }
            }
        }

        // ---- the board: sidebar and the Desk or a Chat ------------------------------------------------------------
        RowLayout {
            anchors.fill: parent
            spacing: 0
            visible: trommi.signed_in

            Sidebar {
                Layout.preferredWidth: 272
                Layout.fillHeight: true
                trommi: trommi
                theme: theme
                sessions: window.sessions
                openCount: window.desk.length
            }

            Rectangle { Layout.fillHeight: true; width: 1; color: theme.line }

            ColumnLayout {
                Layout.fillWidth: true
                Layout.fillHeight: true
                spacing: 0

                // the top bar: where one is, and whether the stream is live
                Rectangle {
                    Layout.fillWidth: true
                    height: 56
                    color: theme.bg
                    RowLayout {
                        anchors.fill: parent
                        anchors.leftMargin: 28
                        anchors.rightMargin: 20
                        spacing: 10
                        Label {
                            text: window.selectedSession ? window.selectedSession.name : "Desk"
                            font.family: theme.display
                            font.pixelSize: 21
                            font.weight: Font.DemiBold
                            color: theme.fg
                            elide: Text.ElideRight
                            Layout.fillWidth: true
                        }
                        Rectangle { width: 8; height: 8; radius: 4; color: trommi.live ? theme.online : theme.faint }
                        Label {
                            text: trommi.live ? "Live" : trommi.status
                            color: theme.muted
                            font.pixelSize: 12
                        }
                    }
                    Rectangle { anchors.bottom: parent.bottom; width: parent.width; height: 1; color: theme.line }
                }

                Desk {
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    visible: trommi.selected === ""
                    trommi: trommi
                    theme: theme
                    cards: window.desk
                }

                Chat {
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    visible: trommi.selected !== ""
                    trommi: trommi
                    theme: theme
                    items: window.chat
                    session: window.selectedSession
                }
            }
        }

        // a refusal or a failure, said once at the bottom
        Rectangle {
            visible: trommi.error !== "" && trommi.signed_in
            anchors.bottom: parent.bottom
            anchors.horizontalCenter: parent.horizontalCenter
            anchors.bottomMargin: 18
            radius: 10
            color: theme.fg
            width: errorText.implicitWidth + 32
            height: 38
            Label { id: errorText; anchors.centerIn: parent; text: trommi.error; color: theme.bg }
            MouseArea { anchors.fill: parent; onClicked: trommi.error = "" }
        }
    }
}
