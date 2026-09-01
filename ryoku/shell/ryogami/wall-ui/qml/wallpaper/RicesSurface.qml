import QtQuick
import Quickshell
import Quickshell.Io
import ".."
import "../components"

// Rices showcase: browse the installed desktop looks (`ryoku-hub rice list`) as
// cards and apply one. A rice replaces wallpaper + palette + decor + layout, so
// applying goes through a confirm. Folded from the Hub's Appearance > Rices tab.
Item {
    id: root

    property var colors
    property bool browserVisible: false

    property var rices: []
    property string _confirmSlug: ""
    property string _confirmName: ""
    property bool _captureOpen: false

    signal escapePressed()

    clip: true

    visible: browserVisible
    opacity: browserVisible ? 1 : 0
    Behavior on opacity { NumberAnimation { duration: Style.animNormal; easing.type: Easing.OutCubic } }

    implicitHeight: Math.max(300, grid.y + grid.implicitHeight + 24)
    height: browserVisible ? implicitHeight : 0
    Behavior on height { NumberAnimation { duration: Style.animEnter; easing.type: Easing.OutCubic } }

    readonly property color _ink:    colors ? colors.surfaceText : "#e0e2e8"
    readonly property color _inkDim: colors ? colors.surfaceVariantText : "#c2c7cf"
    readonly property color _accent: colors ? colors.primary : Style.fallbackAccent

    onBrowserVisibleChanged: if (browserVisible && rices.length === 0) _load()
    Component.onCompleted: if (browserVisible) _load()
    function _load() { if (listProc.running || rices.length > 0) return; _buf = ""; listProc.running = true }

    property string _buf: ""
    Process {
        id: listProc
        command: ["ryoku-hub", "rice", "list"]
        stdout: SplitParser { splitMarker: ""; onRead: function(d) { root._buf += d } }
        onExited: {
            try { root.rices = JSON.parse(root._buf) || [] } catch (e) { root.rices = [] }
        }
    }

    function _apply(slug) { Quickshell.execDetached(["ryoku-hub", "rice", "apply", slug]) }
    function _capture(name) { Quickshell.execDetached(["ryoku-hub", "rice", "capture", name, "all"]); _captureReload.restart() }
    Timer { id: _captureReload; interval: 1200; onTriggered: { root.rices = []; root._buf = ""; listProc.running = true } }

    MouseArea { anchors.fill: parent }

    // ---- header ----
    Row {
        id: header
        anchors.left: parent.left
        anchors.top: parent.top
        anchors.leftMargin: 16
        anchors.topMargin: 14
        spacing: 8

        FilterButton {
            colors: root.colors
            icon: "\u{f0141}"
            skew: 8
            height: 26 * Config.uiScale
            tooltip: "Back to wallpapers"
            onClicked: root.escapePressed()
            anchors.verticalCenter: parent.verticalCenter
        }
        Text {
            text: "RICES"
            font.family: Style.fontFamily
            font.pixelSize: 12 * Config.uiScale
            font.weight: Font.Medium
            font.letterSpacing: 1.4
            color: root._ink
            anchors.verticalCenter: parent.verticalCenter
        }
        Text {
            text: "庵"
            font.family: Style.fontFamily
            font.pixelSize: 11 * Config.uiScale
            color: Qt.rgba(root._inkDim.r, root._inkDim.g, root._inkDim.b, 0.6)
            anchors.verticalCenter: parent.verticalCenter
        }

        FilterButton {
            colors: root.colors
            label: "SAVE LOOK"
            register: false
            skew: 8
            height: 26 * Config.uiScale
            tooltip: "Capture the current desktop as a new rice"
            onClicked: root._captureOpen = true
            anchors.verticalCenter: parent.verticalCenter
        }
    }

    Text {
        anchors.centerIn: parent
        visible: root.rices.length === 0
        text: "No rices yet. Save a look or import one from the Hub."
        font.family: Style.fontFamily; font.pixelSize: 13 * Config.uiScale
        color: root._inkDim
    }

    // ---- rice card grid ----
    Flow {
        id: grid
        anchors.top: header.bottom
        anchors.horizontalCenter: parent.horizontalCenter
        anchors.topMargin: 14
        width: Math.min(parent.width - 48, 3 * (300 + 12) * Config.uiScale)
        spacing: 12

        Repeater {
            model: root.rices

            Rectangle {
                id: card
                width: 300 * Config.uiScale
                height: 132 * Config.uiScale
                radius: Style.radiusMedium
                color: root.colors ? root.colors.surfaceContainer : "#1d100e"
                readonly property bool _active: modelData.active === true
                border.width: _active ? 2 : 1
                border.color: _active ? root._accent
                    : (_cardMouse.containsMouse ? Qt.rgba(root._ink.r, root._ink.g, root._ink.b, 0.35) : Qt.rgba(root._ink.r, root._ink.g, root._ink.b, 0.14))
                Behavior on border.color { ColorAnimation { duration: Style.animVeryFast } }

                Column {
                    anchors.left: parent.left
                    anchors.right: parent.right
                    anchors.top: parent.top
                    anchors.margins: 12
                    spacing: 5

                    Row {
                        width: parent.width
                        Text {
                            width: parent.width - activeSeal.width
                            elide: Text.ElideRight
                            text: modelData.name || modelData.slug || ""
                            font.family: Style.fontFamily; font.pixelSize: 14 * Config.uiScale; font.weight: Font.Medium
                            color: root._ink
                        }
                        Text {
                            id: activeSeal
                            visible: card._active
                            text: "適用中"
                            font.family: Style.fontFamily; font.pixelSize: 9 * Config.uiScale
                            color: root._accent
                            anchors.verticalCenter: parent.verticalCenter
                        }
                    }

                    Text {
                        text: (modelData.author ? ("by " + modelData.author) : "")
                        font.family: Style.fontFamily; font.pixelSize: 9 * Config.uiScale
                        color: Qt.rgba(root._inkDim.r, root._inkDim.g, root._inkDim.b, 0.8)
                    }

                    Text {
                        width: parent.width
                        wrapMode: Text.WordWrap
                        maximumLineCount: 2
                        elide: Text.ElideRight
                        text: modelData.blurb || ""
                        font.family: Style.fontFamily; font.pixelSize: 10 * Config.uiScale
                        color: root._inkDim
                    }
                }

                // tag pills along the bottom
                Row {
                    anchors.left: parent.left
                    anchors.bottom: parent.bottom
                    anchors.margins: 12
                    spacing: 5
                    Repeater {
                        model: (modelData.tags || []).slice(0, 4)
                        Rectangle {
                            height: 15 * Config.uiScale
                            width: tagTxt.width + 10
                            radius: 3
                            color: Qt.rgba(root._ink.r, root._ink.g, root._ink.b, 0.08)
                            border.width: 1
                            border.color: Qt.rgba(root._ink.r, root._ink.g, root._ink.b, 0.14)
                            Text {
                                id: tagTxt
                                anchors.centerIn: parent
                                text: modelData
                                font.family: Style.fontFamily; font.pixelSize: 8 * Config.uiScale; font.letterSpacing: 0.5
                                color: root._inkDim
                            }
                        }
                    }
                }

                MouseArea {
                    id: _cardMouse
                    anchors.fill: parent
                    hoverEnabled: true
                    cursorShape: Qt.PointingHandCursor
                    onClicked: { root._confirmSlug = "" + modelData.slug; root._confirmName = "" + (modelData.name || modelData.slug) }
                }
            }
        }
    }

    // ---- apply confirm ----
    Rectangle {
        anchors.fill: parent
        visible: root._confirmSlug !== "" || opacity > 0.01
        opacity: root._confirmSlug !== "" ? 1 : 0
        Behavior on opacity { NumberAnimation { duration: Style.animNormal } }
        color: Qt.rgba(0, 0, 0, 0.5)
        z: 50
        MouseArea { anchors.fill: parent; onClicked: root._confirmSlug = "" }

        Rectangle {
            anchors.centerIn: parent
            width: Math.min(parent.width - 80, 420 * Config.uiScale)
            height: confirmCol.implicitHeight + 32
            radius: Style.radiusLarge
            color: root.colors ? root.colors.surface : "#131313"
            border.width: 1
            border.color: Qt.rgba(root._ink.r, root._ink.g, root._ink.b, 0.18)
            MouseArea { anchors.fill: parent }

            Column {
                id: confirmCol
                anchors.left: parent.left; anchors.right: parent.right
                anchors.top: parent.top
                anchors.margins: 16
                spacing: 10

                Text {
                    text: "Apply " + root._confirmName + "?"
                    font.family: Style.fontFamily; font.pixelSize: 14 * Config.uiScale; font.weight: Font.Medium
                    color: root._ink
                }
                Text {
                    width: parent.width
                    wrapMode: Text.WordWrap
                    text: "This replaces your wallpaper, colour scheme, decorations and window layout. Your current look is saved so you can revert from the Hub."
                    font.family: Style.fontFamily; font.pixelSize: 11 * Config.uiScale
                    color: root._inkDim
                }
                Row {
                    anchors.right: parent.right
                    spacing: 8
                    FilterButton {
                        colors: root.colors
                        label: "CANCEL"
                        register: false
                        skew: 8
                        height: 28 * Config.uiScale
                        onClicked: root._confirmSlug = ""
                    }
                    FilterButton {
                        colors: root.colors
                        label: "APPLY"
                        register: false
                        skew: 8
                        height: 28 * Config.uiScale
                        hasActiveColor: true
                        activeColor: root._accent
                        isActive: true
                        onClicked: { root._apply(root._confirmSlug); root._confirmSlug = ""; root.escapePressed() }
                    }
                }
            }
        }
    }

    // ---- capture (save current look) ----
    Rectangle {
        anchors.fill: parent
        visible: root._captureOpen || opacity > 0.01
        opacity: root._captureOpen ? 1 : 0
        Behavior on opacity { NumberAnimation { duration: Style.animNormal } }
        color: Qt.rgba(0, 0, 0, 0.5)
        z: 50
        MouseArea { anchors.fill: parent; onClicked: root._captureOpen = false }

        Rectangle {
            anchors.centerIn: parent
            width: Math.min(parent.width - 80, 420 * Config.uiScale)
            height: capCol.implicitHeight + 32
            radius: Style.radiusLarge
            color: root.colors ? root.colors.surface : "#131313"
            border.width: 1
            border.color: Qt.rgba(root._ink.r, root._ink.g, root._ink.b, 0.18)
            MouseArea { anchors.fill: parent }

            Column {
                id: capCol
                anchors.left: parent.left; anchors.right: parent.right; anchors.top: parent.top
                anchors.margins: 16
                spacing: 10

                Text {
                    text: "Save current look"
                    font.family: Style.fontFamily; font.pixelSize: 14 * Config.uiScale; font.weight: Font.Medium
                    color: root._ink
                }
                Text {
                    width: parent.width; wrapMode: Text.WordWrap
                    text: "Capture your wallpaper, palette, decorations and layout as a new rice you can re-apply later."
                    font.family: Style.fontFamily; font.pixelSize: 11 * Config.uiScale
                    color: root._inkDim
                }
                Rectangle {
                    width: parent.width; height: 30 * Config.uiScale
                    color: root.colors ? Qt.rgba(root.colors.surfaceContainer.r, root.colors.surfaceContainer.g, root.colors.surfaceContainer.b, 0.8) : Qt.rgba(0.15, 0.17, 0.22, 0.8)
                    border.width: capInput.activeFocus ? 2 : 1
                    border.color: capInput.activeFocus ? root._accent : Qt.rgba(root._ink.r, root._ink.g, root._ink.b, 0.2)
                    Text {
                        visible: capInput.text.length === 0
                        anchors.left: parent.left; anchors.leftMargin: 10; anchors.verticalCenter: parent.verticalCenter
                        text: "Rice name"
                        font.family: Style.fontFamily; font.pixelSize: 11 * Config.uiScale
                        color: Qt.rgba(root._inkDim.r, root._inkDim.g, root._inkDim.b, 0.7)
                    }
                    TextInput {
                        id: capInput
                        anchors.fill: parent; anchors.leftMargin: 10; anchors.rightMargin: 10
                        verticalAlignment: TextInput.AlignVCenter
                        font.family: Style.fontFamily; font.pixelSize: 11 * Config.uiScale
                        color: root._ink; clip: true; selectByMouse: true
                        onAccepted: if (text.trim().length > 0) { root._capture(text.trim()); text = ""; root._captureOpen = false }
                    }
                }
                Row {
                    anchors.right: parent.right
                    spacing: 8
                    FilterButton { colors: root.colors; label: "CANCEL"; register: false; skew: 8; height: 28 * Config.uiScale; onClicked: { capInput.text = ""; root._captureOpen = false } }
                    FilterButton {
                        colors: root.colors; label: "SAVE"; register: false; skew: 8; height: 28 * Config.uiScale
                        hasActiveColor: true; activeColor: root._accent; isActive: true
                        onClicked: if (capInput.text.trim().length > 0) { root._capture(capInput.text.trim()); capInput.text = ""; root._captureOpen = false }
                    }
                }
            }
        }
    }
}
