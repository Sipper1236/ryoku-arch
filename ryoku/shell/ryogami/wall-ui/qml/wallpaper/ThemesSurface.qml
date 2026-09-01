import QtQuick
import Quickshell
import Quickshell.Io
import ".."
import "../components"
import "../services"

// Theme showcase: swaps the wallpaper grid for a grid of colour-scheme cards
// (from `ryoku-shell theme catalog`). Each card paints itself in its own palette
// so the grid reads as a wall of themes. Clicking applies via `ryoku-shell theme
// <id>`; a TUNE drawer holds recently-applied and the dynamic matugen knobs.
Item {
  id: root

  property var colors
  property bool browserVisible: false
  property bool tuneOpen: false

  property var catalog: []
  property string activeTheme: ""
  property var recent: []

  signal escapePressed()

  clip: true
  focus: browserVisible
  onBrowserVisibleChanged: if (browserVisible) forceActiveFocus()
  Keys.onEscapePressed: (event) => { root.escapePressed(); event.accepted = true }

  visible: browserVisible
  opacity: browserVisible ? 1 : 0
  Behavior on opacity { NumberAnimation { duration: Style.animNormal; easing.type: Easing.OutCubic } }

  readonly property real _maxH: (Screen.height > 0 ? Screen.height : 1000) - 150 * Config.uiScale
  implicitHeight: Math.min(Math.max(300, header.height + 28 + grid.implicitHeight + 24), _maxH)
  height: browserVisible ? implicitHeight : 0
  Behavior on height { NumberAnimation { duration: Style.animEnter; easing.type: Easing.OutCubic } }

  Component.onCompleted: { _loadRecent(); if (browserVisible) _load() }

  function _load() { if (catProc.running || catalog.length > 0) return; _catBuf = ""; catProc.running = true }
  function _loadRecent() {
    var r = Config._data && Config._data.themeRecent
    root.recent = (r && r.length) ? r : []
  }

  function apply(id) {
    Quickshell.execDetached(["ryoku-shell", "theme", id])
    var r = root.recent.slice().filter(function(x) { return x !== id })
    r.unshift(id)
    root.recent = r.slice(0, 6)
    Config.saveKey("themeRecent", root.recent)
    root.activeTheme = id
  }

  property string _catBuf: ""
  Process {
    id: catProc
    command: ["ryoku-shell", "theme", "catalog"]
    stdout: SplitParser {
      splitMarker: ""
      onRead: data => root._catBuf += data
    }
    onExited: {
      try { root.catalog = JSON.parse(root._catBuf) || [] } catch (e) { root.catalog = [] }
    }
  }

  FileView {
    id: shellFile
    path: Quickshell.env("HOME") + "/.config/ryoku/shell.json"
    watchChanges: true
    onFileChanged: reload()
    onLoaded: {
      try {
        var d = JSON.parse(shellFile.text())
        root.activeTheme = (d.theme && d.theme.theme) || ""
      } catch (e) {}
    }
  }

  MouseArea { anchors.fill: parent }

  function _entryFor(id) {
    for (var i = 0; i < root.catalog.length; i++) if (root.catalog[i].id === id) return root.catalog[i]
    return null
  }

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
      text: "THEMES"
      font.family: Style.fontFamily
      font.pixelSize: 12 * Config.uiScale
      font.weight: Font.Medium
      font.letterSpacing: 1.4
      color: root.colors ? root.colors.surfaceText : "#e0e2e8"
      anchors.verticalCenter: parent.verticalCenter
    }
    Text {
      text: "配色"
      font.family: Style.fontFamily
      font.pixelSize: 11 * Config.uiScale
      color: root.colors ? Qt.rgba(root.colors.surfaceVariantText.r, root.colors.surfaceVariantText.g, root.colors.surfaceVariantText.b, 0.6) : "#8a8f97"
      anchors.verticalCenter: parent.verticalCenter
    }
  }

  FilterButton {
    id: tuneBtn
    anchors.right: parent.right
    anchors.top: parent.top
    anchors.rightMargin: 16
    anchors.topMargin: 12
    colors: root.colors
    label: "TUNE"
    register: false
    skew: 8
    height: 26 * Config.uiScale
    isActive: root.tuneOpen
    tooltip: "Recent · scheme type, mode, contrast"
    onClicked: root.tuneOpen = !root.tuneOpen
    z: 45
  }

  // ---- theme card grid ----
  Flickable {
    id: gridFlick
    anchors.top: header.bottom
    anchors.topMargin: 14
    anchors.left: parent.left
    anchors.right: parent.right
    anchors.bottom: parent.bottom
    anchors.bottomMargin: 12
    clip: true
    contentHeight: grid.implicitHeight
    boundsBehavior: Flickable.StopAtBounds

    Flow {
      id: grid
      anchors.horizontalCenter: parent.horizontalCenter
      width: Math.min(gridFlick.width - 48, 5 * (220 + 10) * Config.uiScale)
      spacing: 10

    Repeater {
      model: root.catalog

      Rectangle {
        id: card
        width: 220 * Config.uiScale
        height: 92 * Config.uiScale
        radius: Style.radiusMedium
        readonly property bool _dyn: modelData.dynamic === true
        readonly property var _sw: modelData.sw || []
        readonly property bool _active: root.activeTheme === modelData.id
        color: _dyn
          ? (root.colors ? root.colors.surfaceContainer : "#1d100e")
          : (_sw.length > 0 ? _sw[0] : "#151515")
        border.width: _active ? 2 : 1
        border.color: _active
          ? (root.colors ? root.colors.primary : Style.fallbackAccent)
          : (root.colors ? Qt.rgba(root.colors.surfaceText.r, root.colors.surfaceText.g, root.colors.surfaceText.b, 0.18) : Qt.rgba(1, 1, 1, 0.14))
        Behavior on border.color { ColorAnimation { duration: Style.animVeryFast } }

        Column {
          anchors.left: parent.left
          anchors.right: parent.right
          anchors.top: parent.top
          anchors.margins: 12
          spacing: 8

          Row {
            width: parent.width
            Text {
              width: parent.width - sealText.width
              elide: Text.ElideRight
              text: modelData.label
              font.family: Style.fontFamily
              font.pixelSize: 13 * Config.uiScale
              font.weight: Font.Medium
              color: card._dyn
                ? (root.colors ? root.colors.surfaceText : "#e0e2e8")
                : (card._sw.length > 1 ? card._sw[1] : "#ffffff")
            }
            Text {
              id: sealText
              text: card._dyn ? "自動" : (modelData.dark ? "夜" : "昼")
              font.family: Style.fontFamily
              font.pixelSize: 10 * Config.uiScale
              color: card._dyn
                ? (root.colors ? Qt.rgba(root.colors.surfaceVariantText.r, root.colors.surfaceVariantText.g, root.colors.surfaceVariantText.b, 0.7) : "#8a8f97")
                : (card._sw.length > 6 ? card._sw[6] : "#888888")
              anchors.verticalCenter: parent.verticalCenter
            }
          }

          // dynamic: hint line; static: accent swatch strip
          Text {
            visible: card._dyn
            text: modelData.id === "Wallpaper" ? "follows the wallpaper" : "system default"
            font.family: Style.fontFamily
            font.pixelSize: 10 * Config.uiScale
            color: root.colors ? root.colors.surfaceVariantText : "#c2c7cf"
          }

          Row {
            visible: !card._dyn
            spacing: 4
            Repeater {
              model: card._sw.length > 1 ? card._sw.slice(1) : []
              Rectangle {
                width: 20 * Config.uiScale
                height: 20 * Config.uiScale
                radius: 3
                color: modelData
                border.width: 1
                border.color: Qt.rgba(0, 0, 0, 0.25)
              }
            }
          }
        }

        MouseArea {
          anchors.fill: parent
          hoverEnabled: true
          cursorShape: Qt.PointingHandCursor
          onClicked: root.apply(modelData.id)
        }
      }
    }
  }
  }

  // ---- scrim + TUNE drawer (recent + matugen knobs) ----
  Rectangle {
    anchors.fill: parent
    color: Qt.rgba(0, 0, 0, 0.45)
    visible: root.tuneOpen || opacity > 0.01
    opacity: root.tuneOpen ? 1 : 0
    Behavior on opacity { NumberAnimation { duration: Style.animNormal } }
    z: 40
    MouseArea { anchors.fill: parent; onClicked: root.tuneOpen = false }
  }

  Rectangle {
    id: tuneDrawer
    width: 420 * Config.uiScale
    anchors.top: parent.top
    anchors.bottom: parent.bottom
    x: root.tuneOpen ? (parent.width - width) : parent.width
    Behavior on x { NumberAnimation { duration: Style.animNormal; easing.type: Easing.OutCubic } }
    z: 46
    clip: true
    color: root.colors ? Qt.rgba(root.colors.surface.r, root.colors.surface.g, root.colors.surface.b, 0.98) : Qt.rgba(0.06, 0.07, 0.09, 0.98)
    border.width: 1
    border.color: root.colors ? Qt.rgba(root.colors.surfaceText.r, root.colors.surfaceText.g, root.colors.surfaceText.b, 0.16) : Qt.rgba(1, 1, 1, 0.12)

    MouseArea { anchors.fill: parent }

    Flickable {
      anchors.fill: parent
      anchors.margins: 14
      contentHeight: tuneCol.implicitHeight
      clip: true
      boundsBehavior: Flickable.StopAtBounds

      Column {
        id: tuneCol
        width: parent.width
        spacing: 12

        Text {
          text: "RECENTLY APPLIED"
          font.family: Style.fontFamily
          font.pixelSize: 11 * Config.uiScale
          font.weight: Font.Medium
          font.letterSpacing: 1.2
          color: root.colors ? root.colors.surfaceVariantText : "#c2c7cf"
        }

        Text {
          visible: root.recent.length === 0
          text: "Nothing yet. Pick a theme to start."
          font.family: Style.fontFamily
          font.pixelSize: 11 * Config.uiScale
          color: root.colors ? Qt.rgba(root.colors.surfaceVariantText.r, root.colors.surfaceVariantText.g, root.colors.surfaceVariantText.b, 0.7) : "#8a8f97"
        }

        Flow {
          width: parent.width
          spacing: 6
          Repeater {
            model: root.recent
            FilterButton {
              colors: root.colors
              label: (root._entryFor(modelData) ? root._entryFor(modelData).label : modelData)
              register: false
              skew: 8
              height: 26 * Config.uiScale
              isActive: root.activeTheme === modelData
              onClicked: root.apply(modelData)
            }
          }
        }

        Rectangle {
          width: parent.width; height: 1
          color: root.colors ? Qt.rgba(root.colors.surfaceText.r, root.colors.surfaceText.g, root.colors.surfaceText.b, 0.2) : Qt.rgba(1, 1, 1, 0.16)
        }

        Text {
          text: "DYNAMIC SCHEME"
          font.family: Style.fontFamily
          font.pixelSize: 11 * Config.uiScale
          font.weight: Font.Medium
          font.letterSpacing: 1.2
          color: root.colors ? root.colors.surfaceVariantText : "#c2c7cf"
        }

        Loader {
          width: parent.width
          active: root.tuneOpen
          visible: active
          source: "settings/ThemeSettings.qml"
          onLoaded: {
            item.colors = Qt.binding(function() { return root.colors })
            item.saveConfigKey = function(k, v) { Config.saveKey(k, v) }
            item.notifyThemeChanged = function(scheme, mode, colorIndex) { DaemonClient.retheme(scheme, mode, colorIndex) }
          }
        }
      }
    }
  }
}
