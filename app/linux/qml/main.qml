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

    // the web board's tokens and tones (Theme.qml); dark only when the person picks it, as on the web
    Theme {
        id: theme
        dark: trommi.theme_mode === "dark"
        sans: plex.name || "IBM Plex Sans"
        display: bricolage.name || "Bricolage Grotesque"
        mono: plexMono.name || "IBM Plex Mono"
    }
    // the web's faces, from the app's resources (app/web/public/fonts): the latin files only. Their latin-ext
    // siblings carry the same family name and none of the basic letters; loaded beside them, Qt draws some words
    // from the wrong file.
    FontLoader { id: plex; source: "../fonts/f7.woff2" }
    FontLoader { id: bricolage; source: "../fonts/f1.woff2" }
    FontLoader { id: plexMono; source: "../fonts/f3.woff2" }
    FontLoader { source: "../fonts/f5.woff2" }

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
    property var board: { try { return JSON.parse(trommi.sessions_json) } catch (e) { return ({ desks: [], sessions: [] }) } }
    property var sessions: board.sessions || []
    /** The card whose own page is open (its id), or "". */
    property string openCard: ""
    property var openCardData: {
        for (let i = 0; i < desk.length; i++) if (desk[i].id === openCard) return desk[i]
        return null
    }
    Connections { target: trommi; function onSelectedChanged() { window.openCard = "" } }
    function sessionOf(id) {
        for (let i = 0; i < sessions.length; i++) if (sessions[i].id === id) return sessions[i]
        return null
    }
    property var chat: { try { return JSON.parse(trommi.chat_json) } catch (e) { return [] } }
    property var files: { try { return JSON.parse(trommi.files_json) } catch (e) { return ({}) } }
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
                // (TROMMI_LINUX_FILE: a file of this machine goes with the line)
                if (trommi.scene_file !== "") trommi.send_with_files("The numbers, from the Linux app.", JSON.stringify(["file://" + trommi.scene_file]))
                else trommi.send("Looks good from the Linux app. Ship the split hero, please.")
                shotTimer.restart()
                return
            }
            if (scene === "card" && step === 0) {
                step = 1
                for (let i = 0; i < window.desk.length; i++) if ((window.desk[i].options || []).length > 2) { window.openCard = window.desk[i].id; break }
                if (window.openCard === "" && window.desk.length) window.openCard = window.desk[0].id
                shotTimer.restart()
                return
            }
            // `files` opens the first card with pictures, `save` the first with another file and puts that into Downloads
            if ((scene === "files" || scene === "save") && step === 0) {
                step = 1
                for (let i = 0; i < window.desk.length; i++) {
                    const fs = window.desk[i].files || []
                    const other = fs.filter(f => !f.image)
                    if (scene === "files" ? fs.some(f => f.image) : other.length > 0) {
                        window.openCard = window.desk[i].id
                        if (scene === "save") trommi.save_file(JSON.stringify(other[0].ref))
                        break
                    }
                }
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

        // ---- the board: the sidebar, and the Desk, a card or a Chat ----------------------------------------------
        RowLayout {
            anchors.fill: parent
            spacing: 0
            visible: trommi.signed_in

            Sidebar {
                Layout.preferredWidth: 272
                Layout.fillHeight: true
                trommi: trommi
                theme: theme
                board: window.board
                onHome: window.openCard = ""
            }

            Rectangle {
                Layout.fillWidth: true
                Layout.fillHeight: true
                color: theme.surface

                Desk {
                    anchors.fill: parent
                    visible: trommi.selected === "" && !window.openCardData
                    trommi: trommi
                    theme: theme
                    cards: window.desk
                    sessions: window.sessions
                    files: window.files
                    onOpenCard: (id) => window.openCard = id
                }
                CardView {
                    anchors.fill: parent
                    visible: trommi.selected === "" && !!window.openCardData
                    trommi: trommi
                    theme: theme
                    card: window.openCardData || ({})
                    from: window.openCardData ? window.sessionOf(window.openCardData.session_id) : null
                    files: window.files
                    onBack: window.openCard = ""
                }
                Chat {
                    anchors.fill: parent
                    visible: trommi.selected !== ""
                    trommi: trommi
                    theme: theme
                    items: window.chat
                    session: window.selectedSession
                    files: window.files
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
