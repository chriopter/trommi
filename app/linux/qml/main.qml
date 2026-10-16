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
        // started, the core is told the page and the team last chosen on this device
        Component.onCompleted: { trommi.start(); window.go(window.place) }
    }

    // the app core's view model, as the shell folded it (shell.rs)
    property var view: { try { return JSON.parse(trommi.view_json) } catch (e) { return ({}) } }
    /** The page in view, as last sent to the core (`Event::Focus`): { screen: "desk" | "session" | "card", … }. */
    property var place: ({ screen: "desk" })
    function go(screen) {
        window.place = screen
        const focus = { screen: screen, visible: window.active }
        // the team in view (kept on this device, bridge.rs `team`); none: the core shows the first
        if (trommi.team !== "") focus.team = trommi.team
        trommi.send_event(JSON.stringify({ type: "focus", focus: focus }))
    }
    /** The person chose a team in the sidebar: kept for the next start, and the core told with the page in view. */
    function chooseTeam(id) {
        trommi.choose_team(id)
        go(place)
    }
    /** A tap on the core's view: an `Action`, in the contract's spelling. */
    function act(action) { trommi.send_event(JSON.stringify({ type: "action", action: action })) }
    readonly property var placeSession: place.screen === "session" && view.sessions ? (view.sessions[place.session_id] || null) : null
    readonly property var placeCard: place.screen === "card" && view.cards ? (view.cards[place.card_id] || null) : null
    // which of the window's faces shows: the wait, the sign-in, the board
    readonly property bool starting: view.phase === undefined || view.phase === "starting"
    readonly property bool signedIn: ["opening", "live", "offline"].indexOf(view.phase) >= 0

    // in front or not: the core is told with the page in view
    onActiveChanged: go(place)
    // a notification clicked: what it names (a card, or the Desk), in front
    Connections {
        target: trommi
        function onOpen_requestChanged() {
            const id = trommi.open_request
            if (id === "") return
            window.go(id === "desk" ? { screen: "desk" } : { screen: "card", card_id: id })
            window.show(); window.raise(); window.requestActivate()
        }
    }
    // the time zone: the core is told when it changes (travel, summer time)
    property int zone: -new Date().getTimezoneOffset()
    Timer {
        interval: 60000; repeat: true; running: true
        onTriggered: {
            const now = -new Date().getTimezoneOffset()
            if (now !== window.zone) {
                window.zone = now
                trommi.send_event(JSON.stringify({ type: "zone", utc_offset_min: now }))
            }
        }
    }

    // ---- a scene for pictures and the smoke test: TROMMI_LINUX_SHOT=<file.png> [TROMMI_LINUX_SCENE=card|session|answer]
    Timer {
        id: shotTimer
        interval: Math.max(200, trommi.shot_after_ms)
        running: trommi.shot_path !== "" && !window.starting
        // `card` opens the first row's card, `session` the first session, `answer` takes the first row's first
        // tile; then the picture
        property int step: 0
        onTriggered: {
            const scene = trommi.scene
            if (step === 0 && ["card", "session", "answer"].indexOf(scene) >= 0) {
                step = 1
                const rows = (window.view.desk || {}).rows || []
                const sessions = (window.view.sidebar || {}).sessions || []
                if (scene === "card" && rows.length) window.go({ screen: "card", card_id: rows[0].id })
                if (scene === "session" && sessions.length) window.go({ screen: "session", session_id: sessions[0].id })
                if (scene === "answer" && rows.length && rows[0].tiles.tiles.length) window.act({ do: "answer", card_id: rows[0].id, choices: [rows[0].tiles.tiles[0].key] })
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
            visible: window.view.phase === "signed_out"
            theme: theme
            account: window.view.account || ({})
            status: window.view.status || null
            onAct: (action) => window.act(action)
        }

        // ---- starting --------------------------------------------------------------------------------------------
        Item {
            anchors.fill: parent
            visible: window.starting
            Column {
                anchors.centerIn: parent
                spacing: 12
                BusyIndicator { anchors.horizontalCenter: parent.horizontalCenter; running: window.starting }
                Label { text: window.view.status ? window.view.status.en : ""; color: theme.muted; anchors.horizontalCenter: parent.horizontalCenter }
            }
        }

        // ---- the board: the sidebar, and the Desk, a session or a card ----------------------------------------------
        RowLayout {
            anchors.fill: parent
            spacing: 0
            visible: window.signedIn

            ViewSidebar {
                Layout.preferredWidth: 272
                Layout.fillHeight: true
                theme: theme
                sidebar: window.view.sidebar || ({})
                onTheme_toggled: trommi.set_theme(theme.dark ? "light" : "dark")
                onGo: (screen) => window.go(screen)
                onTeam_chosen: (id) => window.chooseTeam(id)
            }

            Rectangle {
                Layout.fillWidth: true
                Layout.fillHeight: true
                color: theme.surface

                ViewDesk {
                    anchors.fill: parent
                    visible: !window.placeSession && !window.placeCard
                    theme: theme
                    desk: window.view.desk || ({})
                    sidebar: window.view.sidebar || ({})
                    onAct: (action) => window.act(action)
                    onGo: (screen) => window.go(screen)
                }
                ViewSession {
                    anchors.fill: parent
                    visible: !!window.placeSession
                    theme: theme
                    trommi: trommi
                    files: window.view.files || ({})
                    session: window.placeSession || ({})
                    onAct: (action) => window.act(action)
                    onGo: (screen) => window.go(screen)
                }
                ViewCard {
                    anchors.fill: parent
                    visible: !!window.placeCard
                    theme: theme
                    card: window.placeCard || ({})
                    onAct: (action) => window.act(action)
                    onGo: (screen) => window.go(screen)
                }
            }
        }

        // a failure of the window's own (a file not read, the slot not opened), said once at the bottom
        Rectangle {
            visible: trommi.error !== ""
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
