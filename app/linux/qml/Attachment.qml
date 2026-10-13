import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import "pen.mjs" as Pen

// One file a card or a line names: a picture shown (fetched and decrypted by the engine, then from this user's own
// folder), any other file as a chip with its name and size: a click puts it into Downloads, the next one opens it.
Item {
    id: root
    required property var trommi
    required property var theme
    /** The file as the views give it (view.rs files_of): id, name, media_type, size, image, ref. */
    required property var file
    /** Where fetched files lie: attachment id to file URL ("" while it comes). */
    required property var files
    /** The largest a picture is drawn. */
    property real maxWidth: 560
    property real maxHeight: 380

    readonly property string url: files[file.id] || ""
    /** The copy in Downloads: its file URL, "" while it is saved, undefined before. */
    readonly property var savedUrl: { try { return JSON.parse(trommi.saved_json)[file.id] } catch (e) { return undefined } }
    function fetch() { trommi.fetch_file(JSON.stringify(file.ref)) }
    function sizeText(n) {
        if (!n) return ""
        return n < 1024 ? n + " B" : n < 1048576 ? Math.round(n / 1024) + " KB" : (n / 1048576).toFixed(1) + " MB"
    }
    Component.onCompleted: if (file.image && !url) fetch()

    implicitWidth: file.image ? pic.width : chip.implicitWidth
    implicitHeight: file.image ? pic.height : chip.implicitHeight

    // a picture: as wide as it may be, its own proportions
    Rectangle {
        id: pic
        visible: root.file.image
        readonly property real ratio: root.file.width && root.file.height ? root.file.width / root.file.height : (img.status === Image.Ready ? img.implicitWidth / img.implicitHeight : 1.6)
        width: Math.min(root.maxWidth, root.maxHeight * ratio)
        height: width / ratio
        radius: 8
        color: theme.sunken
        border.color: theme.line
        clip: true
        Image {
            id: img
            anchors.fill: parent
            anchors.margins: 1
            source: root.url
            fillMode: Image.PreserveAspectFit
            asynchronous: true
            mipmap: true
        }
        BusyIndicator { anchors.centerIn: parent; running: !root.url; visible: running; width: 28; height: 28 }
        MouseArea { anchors.fill: parent; cursorShape: Qt.PointingHandCursor; onClicked: if (root.url) Qt.openUrlExternally(root.url) }
    }

    // any other file: a chip that opens it
    Rectangle {
        id: chip
        visible: !root.file.image
        implicitWidth: chipRow.implicitWidth + 24
        implicitHeight: 44
        radius: 10
        color: theme.surface
        border.color: hover.hovered ? theme.fg : theme.lineStrong
        HoverHandler { id: hover; cursorShape: Qt.PointingHandCursor }
        RowLayout {
            id: chipRow
            anchors.centerIn: parent
            spacing: 8
            PenMark { Layout.preferredWidth: 20; Layout.preferredHeight: 20; svg: Pen.sketchSvg("clip"); ink: theme.muted; lineWidth: 1.8 }
            Label { text: root.file.name || "file"; font.weight: Font.DemiBold; color: theme.fg; elide: Text.ElideMiddle; Layout.maximumWidth: 280 }
            Label {
                text: root.savedUrl ? "In Downloads" : root.savedUrl === "" ? "Saving …" : root.sizeText(root.file.size)
                color: root.savedUrl ? theme.accent : theme.faint; font.pixelSize: 12
            }
        }
        MouseArea {
            anchors.fill: parent
            onClicked: {
                if (root.savedUrl) Qt.openUrlExternally(root.savedUrl)
                else if (root.savedUrl === undefined) root.trommi.save_file(JSON.stringify(root.file.ref))
            }
        }
    }
}
