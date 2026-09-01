import QtQuick
import QtQuick.Controls
import ".."
import "../components"
import "../services"

// Generic remote-source browser: a search field over a thumbnail grid, backed
// by a RyowallsSource (moewalls / motionbgs / ryostore). Clicking a tile
// downloads the master into the video library and applies it through the
// ryogami daemon. One pane serves every ryowalls-shaped source, so Browse is
// extensible without a bespoke browser each.
Item {
    id: browser

    property var colors
    property bool browserVisible: false
    property string sourceName: ""
    property string kana: ""
    property string searchVerb: ""
    property string downloadVerb: ""
    property bool needsPost: false
    property var extraArgs: []
    property bool searchable: true
    property string idleHint: ""

    signal escapePressed()

    implicitHeight: 640 * Config.uiScale

    readonly property color _ink:    colors ? colors.surfaceText : "#e0e2e8"
    readonly property color _inkDim: colors ? colors.surfaceVariantText : "#c2c7cf"
    readonly property color _line:   colors ? colors.outline : Qt.rgba(1, 1, 1, 0.22)
    readonly property color _accent: colors ? colors.primary : Style.fallbackAccent

    property string _applied: ""      // last-applied id, for a brief tick

    RyowallsSource {
        id: svc
        searchVerb: browser.searchVerb
        downloadVerb: browser.downloadVerb
        needsPost: browser.needsPost
        extraArgs: browser.extraArgs
        onApplied: function(path) {
            DaemonClient.applyVideo(path, [], [], [], null, null, function(res, err) {
                if (!err) browser._applied = path
            })
        }
    }

    // Coalesce the reload triggers (visible / verb / repo / searchable all
    // react to the same source switch) so one search runs after they settle,
    // never with a half-updated repo arg.
    Timer { id: reloadTimer; interval: 60; onTriggered: { if (browser.searchable) svc.search(""); else svc.results = [] } }
    function _reload() { if (browserVisible) reloadTimer.restart(); else svc.results = [] }
    onBrowserVisibleChanged: if (browserVisible) _reload()
    onSearchVerbChanged: _reload()
    onExtraArgsChanged: _reload()
    onSearchableChanged: _reload()
    Component.onCompleted: if (browserVisible) _reload()

    // ---- header: search field ----
    Row {
        id: header
        anchors.top: parent.top
        anchors.horizontalCenter: parent.horizontalCenter
        anchors.topMargin: 12
        spacing: 8

        Rectangle {
            width: 320 * Config.uiScale
            height: 26 * Config.uiScale
            color: browser.colors ? Qt.rgba(browser.colors.surface.r, browser.colors.surface.g, browser.colors.surface.b, 0.8) : Qt.rgba(0.15, 0.17, 0.22, 0.8)
            border.width: searchInput.activeFocus ? 2 : 1
            border.color: searchInput.activeFocus ? browser._accent : browser._line

            Text {
                visible: searchInput.text.length === 0
                anchors.left: parent.left; anchors.leftMargin: 10; anchors.verticalCenter: parent.verticalCenter
                text: "SEARCH " + browser.sourceName.toUpperCase() + "\u2026"
                font.family: Style.fontFamily; font.pixelSize: 11 * Config.uiScale; font.letterSpacing: 1
                color: Qt.rgba(browser._inkDim.r, browser._inkDim.g, browser._inkDim.b, 0.7)
            }
            TextInput {
                id: searchInput
                anchors.fill: parent
                anchors.leftMargin: 10; anchors.rightMargin: 10
                verticalAlignment: TextInput.AlignVCenter
                font.family: Style.fontFamily; font.pixelSize: 11 * Config.uiScale
                color: browser._ink
                clip: true
                selectByMouse: true
                onAccepted: svc.search(text.trim())
            }
        }

        FilterButton {
            colors: browser.colors
            label: svc.loading ? "\u2026" : "SEARCH"
            register: false
            skew: 8
            height: 26 * Config.uiScale
            onClicked: svc.search(searchInput.text.trim())
        }
    }

    // ---- status line (loading / empty / error) ----
    Text {
        anchors.centerIn: parent
        visible: svc.loading || svc.results.length === 0
        text: !browser.searchable ? browser.idleHint : (svc.loading ? "loading\u2026" : (svc.error ? svc.error : "nothing here"))
        font.family: Style.fontFamily; font.pixelSize: 13 * Config.uiScale
        color: browser._inkDim
    }

    // ---- results grid ----
    Flickable {
        id: flick
        anchors.top: header.bottom
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.bottom: parent.bottom
        anchors.topMargin: 12
        anchors.leftMargin: 16
        anchors.rightMargin: 16
        anchors.bottomMargin: 12
        contentHeight: grid.implicitHeight
        clip: true
        boundsBehavior: Flickable.StopAtBounds
        ScrollBar.vertical: ScrollBar {}

        Flow {
            id: grid
            width: flick.width
            spacing: 10

            Repeater {
                model: svc.results

                Rectangle {
                    id: card
                    width: 248 * Config.uiScale
                    height: 152 * Config.uiScale
                    color: browser.colors ? browser.colors.surfaceContainer : "#1d100e"
                    radius: Style.radiusMedium
                    clip: true
                    border.width: 1
                    border.color: _cardMouse.containsMouse ? browser._accent : Qt.rgba(browser._ink.r, browser._ink.g, browser._ink.b, 0.14)
                    Behavior on border.color { ColorAnimation { duration: Style.animVeryFast } }

                    readonly property bool _downloading: svc.downloadingId === ("" + modelData.id)

                    Image {
                        id: thumb
                        anchors.fill: parent
                        anchors.margins: 1
                        source: modelData.thumb || ""
                        fillMode: Image.PreserveAspectCrop
                        asynchronous: true
                        cache: true
                        RotationAnimation on rotation {
                            from: 0; to: 360; duration: Style.animSpin; loops: Animation.Infinite
                            running: thumb.status === Image.Loading
                            target: thumbSpin
                        }
                    }
                    Text {
                        id: thumbSpin
                        anchors.centerIn: parent
                        visible: thumb.status !== Image.Ready
                        text: thumb.status === Image.Error ? "\u{f0028}" : "\u{f0770}"
                        font.family: Style.fontFamilyNerdIcons; font.pixelSize: 18 * Config.uiScale
                        color: browser._inkDim
                    }

                    // bottom scrim + name
                    Rectangle {
                        anchors.left: parent.left; anchors.right: parent.right; anchors.bottom: parent.bottom
                        height: 40 * Config.uiScale
                        gradient: Gradient {
                            GradientStop { position: 0.0; color: "transparent" }
                            GradientStop { position: 1.0; color: Qt.rgba(0, 0, 0, 0.72) }
                        }
                    }
                    Text {
                        anchors.left: parent.left; anchors.right: parent.right; anchors.bottom: parent.bottom
                        anchors.margins: 7
                        text: modelData.name || modelData.id || ""
                        elide: Text.ElideRight
                        font.family: Style.fontFamily; font.pixelSize: 11 * Config.uiScale; font.weight: Font.Medium
                        color: "#ffffff"
                    }

                    // resolution badge
                    Rectangle {
                        visible: (modelData.resolution || "") !== ""
                        anchors.top: parent.top; anchors.right: parent.right; anchors.margins: 6
                        width: resTxt.width + 10; height: resTxt.height + 5
                        radius: 3
                        color: Qt.rgba(0, 0, 0, 0.6)
                        Text {
                            id: resTxt
                            anchors.centerIn: parent
                            text: modelData.resolution || ""
                            font.family: Style.fontFamilyCode; font.pixelSize: 8 * Config.uiScale
                            color: "#ffffff"
                        }
                    }

                    // downloading / applied overlay
                    Rectangle {
                        anchors.fill: parent
                        visible: card._downloading || (browser._applied !== "" && svc.downloadingId === "")
                        color: Qt.rgba(0, 0, 0, card._downloading ? 0.55 : 0)
                        Text {
                            anchors.centerIn: parent
                            visible: card._downloading
                            text: "applying\u2026"
                            font.family: Style.fontFamily; font.pixelSize: 12 * Config.uiScale; font.weight: Font.Medium
                            color: "#ffffff"
                        }
                    }

                    MouseArea {
                        id: _cardMouse
                        anchors.fill: parent
                        hoverEnabled: true
                        cursorShape: Qt.PointingHandCursor
                        enabled: svc.downloadingId === ""
                        onClicked: svc.download(modelData)
                    }
                }
            }
        }
    }

    // back to wallpapers
    FilterButton {
        anchors.left: parent.left
        anchors.top: parent.top
        anchors.leftMargin: 12
        anchors.topMargin: 12
        colors: browser.colors
        icon: "\u{f0141}"
        skew: 8
        height: 26 * Config.uiScale
        tooltip: "Back to wallpapers"
        onClicked: browser.escapePressed()
        z: 20
    }
}
