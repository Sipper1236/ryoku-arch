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

  // ryowalls-shaped remote sources (scrape-based, no keys): one generic
  // LibraryBrowser serves them all, keyed off these descriptors.
  readonly property var _libDefs: ({
    "moewalls":  { label: "MOEWALLS",  kana: "萌", search: "moewalls-search",  download: "moewalls-download",  post: true },
    "motionbgs": { label: "MOTIONBGS", kana: "動", search: "motionbgs-search", download: "motionbgs-download", post: false },
    "ryostore":  { label: "RYOSTORE",  kana: "蔵", search: "extras-search",     download: "extras-download",     post: false },
    "repos":     { label: "REPOS",     kana: "庫", search: "library-list",      download: "library-download",    post: false }
  })
  readonly property bool _isLib: _libDefs[source] !== undefined

  readonly property var _sources: {
    var s = []
    if (Config.wallhavenEnabled) s.push({ key: "wallhaven", label: "WALLHAVEN" })
    if (Config.steamEnabled) s.push({ key: "steam", label: "STEAM" })
    s.push({ key: "moewalls", label: "MOEWALLS" })
    s.push({ key: "motionbgs", label: "MOTIONBGS" })
    s.push({ key: "ryostore", label: "RYOSTORE" })
    s.push({ key: "repos", label: "REPOS" })
    return s
  }
  function _validSource(k) {
    for (var i = 0; i < _sources.length; i++) if (_sources[i].key === k) return true
    return false
  }
  property var repos: []
  property string activeRepo: ""
  function _loadRepos() {
    var r = (Config._data.sources && Config._data.sources.repos) || []
    repos = r.slice()
    if (activeRepo === "" && repos.length > 0) activeRepo = repos[0]
  }
  function _addRepo(raw) {
    var v = ("" + raw).trim().replace(/^https?:\/\/github\.com\//, "").replace(/\.git$/, "").replace(/\/+$/, "")
    if (!v || v.indexOf("/") < 0) return
    if (repos.indexOf(v) < 0) { var n = repos.slice(); n.push(v); repos = n; Config.saveKey("sources.repos", n) }
    activeRepo = v
  }
  function _removeRepo(v) {
    var n = repos.filter(function(x) { return x !== v }); repos = n; Config.saveKey("sources.repos", n)
    if (activeRepo === v) activeRepo = n.length > 0 ? n[0] : ""
  }
  Component.onCompleted: {
    _loadRepos()
    if (_sources.length > 0 && !_validSource(source)) source = _sources[0].key
  }

  visible: browserVisible
  opacity: browserVisible ? 1 : 0
  Behavior on opacity { NumberAnimation { duration: Style.animNormal; easing.type: Easing.OutCubic } }

  readonly property real _browserHeight: {
    if (source === "wallhaven") return whLoader.item ? whLoader.item.implicitHeight : 0
    if (source === "steam") return swLoader.item ? swLoader.item.implicitHeight : 0
    return libLoader.item ? libLoader.item.implicitHeight : 0
  }

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

  Loader {
    id: libLoader
    anchors.fill: parent
    active: root._isLib
    visible: active
    sourceComponent: Component {
      LibraryBrowser {
        colors: root.colors
        browserVisible: true
        sourceName: root._libDefs[root.source] ? root._libDefs[root.source].label : ""
        kana: root._libDefs[root.source] ? root._libDefs[root.source].kana : ""
        searchVerb: root._libDefs[root.source] ? root._libDefs[root.source].search : ""
        downloadVerb: root._libDefs[root.source] ? root._libDefs[root.source].download : ""
        needsPost: root._libDefs[root.source] ? root._libDefs[root.source].post : false
        extraArgs: root.source === "repos" && root.activeRepo !== "" ? ["--repo", root.activeRepo] : []
        searchable: root.source !== "repos" || root.activeRepo !== ""
        idleHint: "Add a GitHub repo in the Sources drawer (owner/repo)."
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
          text: {
            var d = root._libDefs[root.source]
            if (d) return d.label + " · NO KEYS NEEDED"
            return (root.source === "steam" ? "STEAM" : "WALLHAVEN") + " · KEYS & SETUP"
          }
          font.family: Style.fontFamily
          font.pixelSize: 11 * Config.uiScale
          font.weight: Font.Medium
          font.letterSpacing: 1.2
          color: root.colors ? root.colors.surfaceVariantText : "#c2c7cf"
        }

        Text {
          visible: root._isLib
          width: parent.width
          wrapMode: Text.WordWrap
          text: "Scraped source. Browse and click to apply; downloads land in your video library."
          font.family: Style.fontFamily
          font.pixelSize: 10 * Config.uiScale
          color: root.colors ? Qt.rgba(root.colors.surfaceVariantText.r, root.colors.surfaceVariantText.g, root.colors.surfaceVariantText.b, 0.7) : "#8a8f97"
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

        // user GitHub repos: add / select / remove
        Column {
          visible: root.source === "repos"
          width: parent.width
          spacing: 8

          Flow {
            width: parent.width
            spacing: 6
            visible: root.repos.length > 0
            Repeater {
              model: root.repos
              Row {
                spacing: 2
                FilterButton {
                  colors: root.colors
                  label: modelData
                  register: false
                  skew: 8
                  height: 26 * Config.uiScale
                  isActive: root.activeRepo === modelData
                  onClicked: root.activeRepo = modelData
                }
                FilterButton {
                  colors: root.colors
                  label: "\u00d7"
                  register: false
                  skew: 8
                  height: 26 * Config.uiScale
                  onClicked: root._removeRepo(modelData)
                }
              }
            }
          }

          Row {
            width: parent.width
            spacing: 6
            Rectangle {
              width: parent.width - addBtn.width - parent.spacing
              height: 28 * Config.uiScale
              color: root.colors ? Qt.rgba(root.colors.surface.r, root.colors.surface.g, root.colors.surface.b, 0.8) : Qt.rgba(0.15, 0.17, 0.22, 0.8)
              border.width: repoInput.activeFocus ? 2 : 1
              border.color: repoInput.activeFocus ? (root.colors ? root.colors.primary : Style.fallbackAccent) : (root.colors ? Qt.rgba(root.colors.surfaceText.r, root.colors.surfaceText.g, root.colors.surfaceText.b, 0.2) : Qt.rgba(1, 1, 1, 0.16))
              Text {
                visible: repoInput.text.length === 0
                anchors.left: parent.left; anchors.leftMargin: 10; anchors.verticalCenter: parent.verticalCenter
                text: "owner/repo"
                font.family: Style.fontFamily; font.pixelSize: 11 * Config.uiScale
                color: root.colors ? Qt.rgba(root.colors.surfaceVariantText.r, root.colors.surfaceVariantText.g, root.colors.surfaceVariantText.b, 0.7) : "#8a8f97"
              }
              TextInput {
                id: repoInput
                anchors.fill: parent; anchors.leftMargin: 10; anchors.rightMargin: 10
                verticalAlignment: TextInput.AlignVCenter
                font.family: Style.fontFamily; font.pixelSize: 11 * Config.uiScale
                color: root.colors ? root.colors.surfaceText : "#e0e2e8"
                clip: true; selectByMouse: true
                onAccepted: { root._addRepo(text); text = "" }
              }
            }
            FilterButton {
              id: addBtn
              colors: root.colors
              label: "ADD"
              register: false
              skew: 8
              height: 28 * Config.uiScale
              onClicked: { root._addRepo(repoInput.text); repoInput.text = "" }
            }
          }
        }
      }
    }
  }
}
