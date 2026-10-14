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
    // a notification clicked: its card (or the Desk), in front
    onActiveChanged: if (core) go(place); else trommi.set_focused(active)
    Connections {
        target: trommi
        function onOpen_requestChanged() {
            const id = trommi.open_request
            if (id === "") return
            // on the app core: a notification names a card (or nothing: the Desk)
            if (window.core) {
                window.go(id === "desk" ? { screen: "desk" } : { screen: "card", card_id: id })
                window.show(); window.raise(); window.requestActivate()
                return
            }
            if (trommi.selected !== "") trommi.select_session("")
            window.openCard = id === "desk" ? "" : id
            window.show(); window.raise(); window.requestActivate()
        }
    }
    function sessionOf(id) {
        for (let i = 0; i < sessions.length; i++) if (sessions[i].id === id) return sessions[i]
        return null
    }
    property var chat: { try { return JSON.parse(trommi.chat_json) } catch (e) { return [] } }
    property var files: { try { return JSON.parse(trommi.files_json) } catch (e) { return ({}) } }
    // the app core's view model, as the shell folded it (shell.rs); `core`: it is the one behind this window
    property var view: { try { return JSON.parse(trommi.view_json) } catch (e) { return ({}) } }
    readonly property bool core: view.phase !== undefined
    /** The page in view, as last sent to the core (`Event::Focus`): { screen: "desk" | "session" | "card", … }. */
    property var place: ({ screen: "desk" })
    function go(screen) {
        window.place = screen
        trommi.send_event(JSON.stringify({ type: "focus", focus: { screen: screen, visible: window.active } }))
    }
    // the time zone: the core is told when it changes (travel, summer time)
    property int zone: -new Date().getTimezoneOffset()
    Timer {
        interval: 60000; repeat: true; running: window.core
        onTriggered: {
            const now = -new Date().getTimezoneOffset()
            if (now !== window.zone) {
                window.zone = now
                trommi.send_event(JSON.stringify({ type: "zone", utc_offset_min: now }))
            }
        }
    }
    /** A tap on the core's view: an `Action`, in the contract's spelling. */
    function act(action) { trommi.send_event(JSON.stringify({ type: "action", action: action })) }
    readonly property var placeSession: place.screen === "session" && view.sessions ? (view.sessions[place.session_id] || null) : null
    readonly property var placeCard: place.screen === "card" && view.cards ? (view.cards[place.card_id] || null) : null
    // which of the window's three faces shows: the wait, the sign-in, the board
    readonly property bool starting: core ? view.phase === "starting" : trommi.starting
    readonly property bool signedIn: core ? ["opening", "live", "offline"].indexOf(view.phase) >= 0 : trommi.signed_in
    property var selectedSession: {
        for (let i = 0; i < sessions.length; i++) if (sessions[i].id === trommi.selected) return sessions[i]
        return null
    }

    // ---- a scene for pictures and the smoke test: TROMMI_LINUX_SHOT=<file.png> [TROMMI_LINUX_SCENE=chat]
    Timer {
        id: shotTimer
        interval: Math.max(200, trommi.shot_after_ms)
        running: trommi.shot_path !== "" && (window.signedIn ? window.core || window.desk.length > 0 || trommi.scene === "empty" : !window.starting)
        // a scene's steps, one per tick: `chat` opens a session (TROMMI_LINUX_SESSION names it), `say` also writes
        // to its agent, `answer` answers the first decision card with its first option; then the picture
        property int step: 0
        onTriggered: {
            const scene = trommi.scene
            // on the app core: `card` opens the first row's card, `session` the first session, `answer` takes the
            // first row's first tile; then the picture
            if (window.core && step === 0 && ["card", "session", "answer"].indexOf(scene) >= 0) {
                step = 1
                const rows = (window.view.desk || {}).rows || []
                const sessions = (window.view.sidebar || {}).sessions || []
                if (scene === "card" && rows.length) window.go({ screen: "card", card_id: rows[0].id })
                if (scene === "session" && sessions.length) window.go({ screen: "session", session_id: sessions[0].id })
                if (scene === "answer" && rows.length && rows[0].tiles.tiles.length) window.act({ do: "answer", card_id: rows[0].id, choices: [rows[0].tiles.tiles[0].key] })
                shotTimer.restart()
                return
            }
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
        ViewSignIn {
            anchors.fill: parent
            visible: window.core && window.view.phase === "signed_out"
            theme: theme
            account: window.view.account || ({})
            status: window.view.status || null
            onAct: (action) => window.act(action)
        }
        SignIn {
            anchors.fill: parent
            visible: !window.core && !window.signedIn && !window.starting
            trommi: trommi
            theme: theme
        }

        // ---- starting --------------------------------------------------------------------------------------------
        Item {
            anchors.fill: parent
            visible: window.starting
            Column {
                anchors.centerIn: parent
                spacing: 12
                BusyIndicator { anchors.horizontalCenter: parent.horizontalCenter; running: window.starting }
                Label { text: trommi.status; color: theme.muted; anchors.horizontalCenter: parent.horizontalCenter }
            }
        }

        // ---- the board: the sidebar, and the Desk, a card or a Chat ----------------------------------------------
        RowLayout {
            anchors.fill: parent
            spacing: 0
            visible: window.signedIn

            // the app core behind the window: the sidebar and the Desk are drawn from its view model
            ViewSidebar {
                visible: window.core
                Layout.preferredWidth: 272
                Layout.fillHeight: true
                theme: theme
                sidebar: window.view.sidebar || ({})
                onTheme_toggled: trommi.set_theme(theme.dark ? "light" : "dark")
                onGo: (screen) => window.go(screen)
            }
            Sidebar {
                visible: !window.core
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

                ViewDesk {
                    anchors.fill: parent
                    visible: window.core && !window.placeSession && !window.placeCard
                    theme: theme
                    desk: window.view.desk || ({})
                    sidebar: window.view.sidebar || ({})
                    onAct: (action) => window.act(action)
                    onGo: (screen) => window.go(screen)
                }
                ViewSession {
                    anchors.fill: parent
                    visible: window.core && !!window.placeSession
                    theme: theme
                    trommi: trommi
                    files: window.view.files || ({})
                    session: window.placeSession || ({})
                    onAct: (action) => window.act(action)
                    onGo: (screen) => window.go(screen)
                }
                ViewCard {
                    anchors.fill: parent
                    visible: window.core && !!window.placeCard
                    theme: theme
                    card: window.placeCard || ({})
                    onAct: (action) => window.act(action)
                    onGo: (screen) => window.go(screen)
                }
                Desk {
                    anchors.fill: parent
                    visible: !window.core && trommi.selected === "" && !window.openCardData
                    trommi: trommi
                    theme: theme
                    cards: window.desk
                    sessions: window.sessions
                    files: window.files
                    onOpenCard: (id) => window.openCard = id
                }
                CardView {
                    anchors.fill: parent
                    visible: !window.core && trommi.selected === "" && !!window.openCardData
                    trommi: trommi
                    theme: theme
                    card: window.openCardData || ({})
                    from: window.openCardData ? window.sessionOf(window.openCardData.session_id) : null
                    files: window.files
                    onBack: window.openCard = ""
                }
                Chat {
                    anchors.fill: parent
                    visible: !window.core && trommi.selected !== ""
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
            visible: trommi.error !== "" && window.signedIn
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
