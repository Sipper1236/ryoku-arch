import QtQuick
import ".."
import "../components"
import "../services"

// One Browse surface for every remote source. The active source's browser fills
// the width; a SOURCES button opens a slide-out drawer that switches source and
// holds that source's keys & setup (API keys, grid), so sources and their config
// live in one place instead of scattered across browser buttons and settings tabs.
Item {
  id: root

  property var colors
  property var whService
  property var swService
  property bool browserVisible: false
  property string source: "wallhaven"     // active source key
  property bool drawerOpen: false

  signal escapePressed()

  function _saveField(key, value) {
    if (!Config._data.components || typeof Config._data.components.wallpaperSelector !== "object" || Config._data.components.wallpaperSelector === null)
      Config.saveKey("components.wallpaperSelector.enabled", true)
    Config.saveKey("components.wallpaperSelector." + key, value)
  }
  function _saveConfigKey(path, value) { Config.saveKey(path, value) }

  readonly property var _sources: {
    var s = []
    if (Config.wallhavenEnabled) s.push({ key: "wallhaven", label: "WALLHAVEN" })
    if (Config.steamEnabled) s.push({ key: "steam", label: "STEAM" })
    return s
  }
  function _validSource(k) {
    for (var i = 0; i < _sources.length; i++) if (_sources[i].key === k) return true
    return false
  }
  Component.onCompleted: if (_sources.length > 0 && !_validSource(source)) source = _sources[0].key

  visible: browserVisible
  opacity: browserVisible ? 1 : 0
  Behavior on opacity { NumberAnimation { duration: Style.animNormal; easing.type: Easing.OutCubic } }

  readonly property real _browserHeight: source === "wallhaven"
    ? (whLoader.item ? whLoader.item.implicitHeight : 0)
    : (swLoader.item ? swLoader.item.implicitHeight : 0)

  implicitHeight: Math.max(240, _browserHeight)
  height: browserVisible ? implicitHeight : 0
  Behavior on height { NumberAnimation { duration: Style.animEnter; easing.type: Easing.OutCubic } }

  // ---- content: the active source's browser, full width ----
  Loader {
    id: whLoader
    anchors.fill: parent
    active: root.source === "wallhaven"
    visible: active
    sourceComponent: Component {
      WallhavenBrowser {
        colors: root.colors
        whService: root.whService
        browserVisible: true
        onEscapePressed: root.escapePressed()
      }
    }
  }

  Loader {
    id: swLoader
    anchors.fill: parent
    active: root.source === "steam"
    visible: active
    sourceComponent: Component {
      SteamWorkshopBrowser {
        colors: root.colors
        swService: root.swService
        browserVisible: true
        onEscapePressed: root.escapePressed()
      }
    }
  }

  // ---- SOURCES toggle (top-left) ----
  FilterButton {
    id: sourcesBtn
    anchors.left: parent.left
    anchors.top: parent.top
    anchors.leftMargin: 12
    anchors.topMargin: 12
    colors: root.colors
    label: "SOURCES"
    register: false
    skew: 8
    height: 26 * Config.uiScale
    isActive: root.drawerOpen
    tooltip: "Switch source · keys & setup"
    onClicked: root.drawerOpen = !root.drawerOpen
    z: 45
  }

  // ---- scrim ----
  Rectangle {
    anchors.fill: parent
    color: Qt.rgba(0, 0, 0, 0.45)
    visible: root.drawerOpen || opacity > 0.01
    opacity: root.drawerOpen ? 1 : 0
    Behavior on opacity { NumberAnimation { duration: Style.animNormal } }
    z: 40
    MouseArea { anchors.fill: parent; onClicked: root.drawerOpen = false }
  }

  // ---- sources & setup drawer ----
  Rectangle {
    id: drawer
    width: 440 * Config.uiScale
    anchors.top: parent.top
    anchors.bottom: parent.bottom
    x: root.drawerOpen ? 0 : -width
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
      contentHeight: drawerCol.implicitHeight
      clip: true
      boundsBehavior: Flickable.StopAtBounds

      Column {
        id: drawerCol
        width: parent.width
        spacing: 10

        Row {
          spacing: 6
          Text {
            text: "SOURCES"
            font.family: Style.fontFamily
            font.pixelSize: 12 * Config.uiScale
            font.weight: Font.Medium
            font.letterSpacing: 1.4
            color: root.colors ? root.colors.surfaceText : "#e0e2e8"
          }
          Text {
            text: "源"
            font.family: Style.fontFamily
            font.pixelSize: 12 * Config.uiScale
            color: root.colors ? Qt.rgba(root.colors.surfaceVariantText.r, root.colors.surfaceVariantText.g, root.colors.surfaceVariantText.b, 0.6) : "#8a8f97"
            anchors.verticalCenter: parent.verticalCenter
          }
        }

        Flow {
          width: parent.width
          spacing: 6
          Repeater {
            model: root._sources
            FilterButton {
              colors: root.colors
              label: modelData.label
              register: false
              skew: 8
              height: 28 * Config.uiScale
              isActive: root.source === modelData.key
              onClicked: { root.source = modelData.key; root.drawerOpen = false }
            }
          }
        }

        Rectangle {
          width: parent.width; height: 1
          color: root.colors ? Qt.rgba(root.colors.surfaceText.r, root.colors.surfaceText.g, root.colors.surfaceText.b, 0.2) : Qt.rgba(1, 1, 1, 0.16)
        }

        Text {
          text: (root.source === "steam" ? "STEAM" : "WALLHAVEN") + " · KEYS & SETUP"
          font.family: Style.fontFamily
          font.pixelSize: 11 * Config.uiScale
          font.weight: Font.Medium
          font.letterSpacing: 1.2
          color: root.colors ? root.colors.surfaceVariantText : "#c2c7cf"
        }

        Loader {
          width: parent.width
          active: root.source === "wallhaven"
          visible: active
          source: "settings/WallhavenSettings.qml"
          onLoaded: {
            item.colors = Qt.binding(function() { return root.colors })
            item.saveField = function(k, v) { root._saveField(k, v) }
            item.saveConfigKey = function(k, v) { root._saveConfigKey(k, v) }
          }
        }

        Loader {
          width: parent.width
          active: root.source === "steam"
          visible: active
          source: "settings/SteamSettings.qml"
          onLoaded: {
            item.colors = Qt.binding(function() { return root.colors })
            item.saveField = function(k, v) { root._saveField(k, v) }
            item.saveConfigKey = function(k, v) { root._saveConfigKey(k, v) }
          }
        }
      }
    }
  }
}
