import QtQuick
import "../kit"
import "../kit/Widgets.js" as Widgets
import "../../modules"
import Ryoku.Ui
import Ryoku.Ui.Singletons

// Widgets route (部品): every catalogue widget as a row you show, size, colour and
// tune. A row carries the widget's glyph, its name and gloss, a PLUGIN tag when
// it is one, and an on/off switch; selecting it expands to its density (icon
// only), its per-widget colour, and its own settings from the catalogue, rendered
// by type through the root barWidget seam. The launcher's mark and the
// workspaces' count/marker (the old Logo and Spaces routes) are just those two
// widgets' settings now, and the workspaces row keeps its live marker preview.
// A `select` arg (Layout's SETTINGS link) opens the panel on a chosen widget.
Item {
    id: page
    property var root: null
    property var cc: null
    readonly property var tk: page.cc ? page.cc.tokens : null
    readonly property real colW: page.width
    implicitHeight: contentCol.implicitHeight

    property string selId: ""      // the expanded widget row
    property string colorGid: ""   // the widget whose colour popover is open
    property string colorLabel: ""

    // Per-widget colour is gated on widgetHasFill so the page stays error-free if
    // the live Theme (or the offscreen probe) does not expose the helpers.
    readonly property bool colorSupported: !!(page.root && page.root.widgetHasFill)

    // Layout's SETTINGS link lands here on a chosen widget: read the pending arg
    // once, then clear it so a later plain open does not re-expand it.
    function consumeArg() {
        if (page.cc && page.cc.routeArg !== null && page.cc.routeArg !== undefined) {
            page.selId = String(page.cc.routeArg)
            page.cc.routeArg = null
        }
    }
    Component.onCompleted: page.consumeArg()
    Connections {
        target: page.cc
        ignoreUnknownSignals: true
        function onRouteArgChanged() { page.consumeArg() }
    }

    // Every setting value goes through the root barWidget seam. Reads are reactive
    // because barWidgetGet reads a live Theme property (or pluginSettings) during
    // evaluation, so a control reflects a change without a manual refresh.
    function getW(id, key) {
        return (page.root && page.root.barWidgetGet) ? page.root.barWidgetGet(id, key) : undefined
    }
    function setW(id, key, val) {
        if (page.root && page.root.barWidgetSet) page.root.barWidgetSet(id, key, val)
    }
    function showW(id, on) {
        if (page.root && page.root.barLayoutShow) page.root.barLayoutShow(id, on)
    }
    // density (icon-only) is keyed by gid, not id.
    function isIcon(gid) { return !!(page.root && page.root.iconOnly && page.root.iconOnly(gid)) }
    function toggleDensity(gid) { if (page.root && page.root.toggleIconOnly) page.root.toggleIconOnly(gid) }

    // one catalogue setting, rendered by type; options may be plain strings
    // (built-in) or {value,label} objects (a plugin manifest).
    component WSetting: Column {
        id: ws
        property string wid: ""
        property var setting: null
        readonly property string skey: ws.setting ? String(ws.setting.key) : ""
        readonly property string stype: ws.setting ? String(ws.setting.type) : ""
        readonly property var opts: ws.optionValues(ws.setting)
        readonly property var optLabels: ws.optionLabelMap(ws.setting)
        readonly property bool allStrings: ws.optionsAllStrings(ws.setting)

        width: parent ? parent.width : 0
        spacing: page.tk ? page.tk.gap / 2 : 6

        function optionValues(s) {
            var o = (s && s.options) ? s.options : []
            var out = []
            for (var i = 0; i < o.length; i++)
                out.push((o[i] && typeof o[i] === "object") ? String(o[i].value) : String(o[i]))
            return out
        }
        function optionLabelMap(s) {
            var o = (s && s.options) ? s.options : []
            var m = ({})
            for (var i = 0; i < o.length; i++)
                if (o[i] && typeof o[i] === "object") m[String(o[i].value)] = String(o[i].label)
            return m
        }
        function optionsAllStrings(s) {
            var o = (s && s.options) ? s.options : []
            for (var i = 0; i < o.length; i++)
                if (o[i] && typeof o[i] === "object") return false
            return true
        }

        UiText {
            text: I18n.tr(ws.setting ? ws.setting.label : "")
            color: Tokens.inkMuted
            font.family: Tokens.mono
            font.pixelSize: Tokens.fTiny
            font.letterSpacing: Tokens.trackLabel
        }

        Component {
            id: segK
            Seg {
                options: ws.opts
                current: { var v = page.getW(ws.wid, ws.skey); return v === undefined ? "" : String(v) }
                onChose: (k) => page.setW(ws.wid, ws.skey, k)
            }
        }
        Component {
            id: chipsK
            Chips {
                width: ws.width
                options: ws.opts
                labels: ws.optLabels
                current: { var v = page.getW(ws.wid, ws.skey); return v === undefined ? "" : String(v) }
                onChose: (k) => page.setW(ws.wid, ws.skey, k)
            }
        }
        Component {
            id: swK
            Sw {
                on: page.getW(ws.wid, ws.skey) === true
                onToggled: (v) => page.setW(ws.wid, ws.skey, v)
            }
        }
        Component {
            id: multiK
            Multi {
                width: ws.width
                options: ws.opts
                chosen: { var v = page.getW(ws.wid, ws.skey); return Array.isArray(v) ? v : [] }
                onToggled: (k) => {
                    var cur = page.getW(ws.wid, ws.skey)
                    var arr = Array.isArray(cur) ? cur.slice() : []
                    var i = arr.indexOf(k)
                    if (i >= 0) arr.splice(i, 1); else arr.push(k)
                    page.setW(ws.wid, ws.skey, arr)
                }
            }
        }
        Component {
            id: stepK
            Step {
                from: (ws.setting && ws.setting.min !== undefined) ? ws.setting.min : 0
                to: (ws.setting && ws.setting.max !== undefined) ? ws.setting.max : 100
                value: { var v = Number(page.getW(ws.wid, ws.skey)); return isNaN(v) ? from : v }
                onModified: (v) => page.setW(ws.wid, ws.skey, v)
            }
        }
        Component {
            id: fieldK
            Field {
                width: Math.min(ws.width, 260)
                tabular: true
                text: { var v = page.getW(ws.wid, ws.skey); return v === undefined ? "" : String(v) }
                onCommitted: (v) => page.setW(ws.wid, ws.skey, v)
            }
        }

        Loader {
            width: ws.width
            sourceComponent: {
                switch (ws.stype) {
                case "toggle": return swK
                case "multi":  return multiK
                case "int":    return stepK
                case "text":   return fieldK
                case "choice": return (ws.allStrings && ws.opts.length <= 4) ? segK : chipsK
                default:       return null
                }
            }
        }
    }

    Flickable {
        id: flick
        anchors.fill: parent
        contentWidth: width
        contentHeight: contentCol.implicitHeight
        clip: true
        boundsBehavior: Flickable.StopAtBounds

        Column {
            id: contentCol
            width: page.colW
            spacing: page.tk ? page.tk.sectionGap : 24

            Entrance {
                width: page.colW
                index: 0
                SettingCard {
                    width: page.colW
                    title: "WIDGETS"
                    kana: "\u90e8\u54c1"

                    Repeater {
                        id: rep
                        model: (page.root && page.root.barCatalog) ? page.root.barCatalog : []

                        delegate: Column {
                            id: wr
                            required property var modelData
                            required property int index
                            readonly property string wid: wr.modelData.id
                            readonly property string gid: wr.modelData.gid || ""
                            readonly property bool isPlugin: wr.modelData.kind === "plugin"
                            readonly property bool expanded: page.selId === wr.wid
                            readonly property var settings: wr.modelData.settings || []
                            readonly property string glyph: Widgets.glyphFor(wr.wid)
                            width: parent ? parent.width : 0

                            // ── the row header ──
                            Rectangle {
                                width: parent.width
                                height: page.tk ? page.tk.rowH : 40
                                color: (headMa.containsMouse || wr.expanded) ? Tokens.tint5 : "transparent"
                                Behavior on color { ColorAnimation { duration: Tokens.snap } }

                                Rectangle {
                                    visible: wr.index > 0
                                    anchors { top: parent.top; left: parent.left; right: parent.right }
                                    anchors.leftMargin: page.tk ? page.tk.pad : 24
                                    anchors.rightMargin: page.tk ? page.tk.pad : 24
                                    height: 1
                                    color: Tokens.lineSoft
                                }

                                IconText {
                                    id: rowGlyph
                                    visible: wr.glyph !== ""
                                    anchors.left: parent.left
                                    anchors.leftMargin: page.tk ? page.tk.pad : 24
                                    anchors.verticalCenter: parent.verticalCenter
                                    text: wr.glyph
                                    color: wr.modelData.shown ? Tokens.inkDim : Tokens.inkFaint
                                    font.pixelSize: Tokens.fRow
                                }
                                UiText {
                                    id: rowInitial
                                    visible: wr.glyph === ""
                                    anchors.left: parent.left
                                    anchors.leftMargin: page.tk ? page.tk.pad : 24
                                    anchors.verticalCenter: parent.verticalCenter
                                    text: Widgets.initialFor(wr.modelData.label)
                                    color: wr.modelData.shown ? Tokens.inkDim : Tokens.inkFaint
                                    font.family: Tokens.mono
                                    font.pixelSize: Tokens.fBody
                                    font.weight: Font.DemiBold
                                }
                                Row {
                                    anchors.left: (wr.glyph !== "" ? rowGlyph.right : rowInitial.right)
                                    anchors.leftMargin: page.tk ? page.tk.gap : 12
                                    anchors.right: tag.left
                                    anchors.rightMargin: page.tk ? page.tk.gap : 12
                                    anchors.verticalCenter: parent.verticalCenter
                                    spacing: page.tk ? page.tk.gap / 2 : 6
                                    UiText {
                                        anchors.verticalCenter: parent.verticalCenter
                                        text: I18n.tr(wr.modelData.label)
                                        color: Tokens.ink
                                        font.family: Tokens.ui
                                        font.pixelSize: Tokens.fRow
                                        font.weight: Font.Medium
                                    }
                                    UiText {
                                        anchors.verticalCenter: parent.verticalCenter
                                        text: wr.modelData.gloss || ""
                                        color: Tokens.inkFaint
                                        font.family: Tokens.jp
                                        font.pixelSize: Tokens.fSmall
                                    }
                                }
                                UiText {
                                    id: tag
                                    anchors.right: caret.left
                                    anchors.rightMargin: page.tk ? page.tk.gap : 12
                                    anchors.verticalCenter: parent.verticalCenter
                                    visible: wr.isPlugin
                                    text: I18n.tr("PLUGIN")
                                    color: Tokens.inkFaint
                                    font.family: Tokens.mono
                                    font.pixelSize: Tokens.fTiny
                                    font.letterSpacing: Tokens.trackLabel
                                }
                                UiText {
                                    id: caret
                                    anchors.right: sw.left
                                    anchors.rightMargin: page.tk ? page.tk.gap : 12
                                    anchors.verticalCenter: parent.verticalCenter
                                    text: "\u25b8"
                                    rotation: wr.expanded ? 90 : 0
                                    color: Tokens.inkFaint
                                    font.family: Tokens.ui
                                    font.pixelSize: 10
                                    Behavior on rotation { NumberAnimation { duration: Tokens.snap; easing.type: Tokens.easeSnap } }
                                }
                                Sw {
                                    id: sw
                                    anchors.right: parent.right
                                    anchors.rightMargin: page.tk ? page.tk.pad : 24
                                    anchors.verticalCenter: parent.verticalCenter
                                    enabled: Widgets.hideable(wr.wid)
                                    on: wr.modelData.shown === true
                                    onToggled: (v) => page.showW(wr.wid, v)
                                }

                                // expand on a click anywhere left of the switch.
                                MouseArea {
                                    id: headMa
                                    anchors.left: parent.left
                                    anchors.right: sw.left
                                    anchors.top: parent.top
                                    anchors.bottom: parent.bottom
                                    hoverEnabled: true
                                    cursorShape: Qt.PointingHandCursor
                                    onClicked: page.selId = wr.expanded ? "" : wr.wid
                                }
                            }

                            // ── the expansion ──
                            Item {
                                width: parent.width
                                clip: true
                                height: wr.expanded ? detail.implicitHeight + (page.tk ? page.tk.gap * 2 : 24) : 0
                                Behavior on height { NumberAnimation { duration: Tokens.swap; easing.type: Tokens.ease } }

                                Column {
                                    id: detail
                                    anchors.left: parent.left
                                    anchors.right: parent.right
                                    anchors.top: parent.top
                                    anchors.topMargin: page.tk ? page.tk.gap : 12
                                    anchors.leftMargin: page.tk ? page.tk.pad : 24
                                    anchors.rightMargin: page.tk ? page.tk.pad : 24
                                    spacing: page.tk ? page.tk.gap : 12

                                    // density (icon-only), for the widgets that have it.
                                    Column {
                                        width: parent.width
                                        visible: Widgets.densitySupported(wr.wid) && wr.gid !== ""
                                        spacing: page.tk ? page.tk.gap / 2 : 6
                                        UiText {
                                            text: I18n.tr("DENSITY")
                                            color: Tokens.inkMuted
                                            font.family: Tokens.mono
                                            font.pixelSize: Tokens.fTiny
                                            font.letterSpacing: Tokens.trackLabel
                                        }
                                        Seg {
                                            options: ["Full", "Icon"]
                                            current: page.isIcon(wr.gid) ? "Icon" : "Full"
                                            onChose: (k) => { if ((k === "Icon") !== page.isIcon(wr.gid)) page.toggleDensity(wr.gid) }
                                        }
                                    }

                                    // per-widget colour: opens the colour popover.
                                    Column {
                                        width: parent.width
                                        visible: page.colorSupported && wr.gid !== ""
                                        spacing: page.tk ? page.tk.gap / 2 : 6
                                        UiText {
                                            text: I18n.tr("COLOUR")
                                            color: Tokens.inkMuted
                                            font.family: Tokens.mono
                                            font.pixelSize: Tokens.fTiny
                                            font.letterSpacing: Tokens.trackLabel
                                        }
                                        Row {
                                            spacing: page.tk ? page.tk.gap : 12
                                            Rectangle {
                                                id: swatch
                                                readonly property bool assigned: page.colorSupported && page.root.widgetHasFill(wr.gid)
                                                readonly property bool menuOpen: page.colorGid === wr.gid
                                                width: Tokens.ctlH
                                                height: Tokens.ctlH
                                                radius: Tokens.radius
                                                color: swatch.assigned ? page.root.widgetAssignedColor(wr.gid)
                                                    : (swatchMa.containsMouse ? Tokens.tint5 : "transparent")
                                                border.width: (swatch.menuOpen || swatch.assigned) ? 2 : 1
                                                border.color: (swatch.menuOpen || swatch.assigned) ? Tokens.bone
                                                    : (swatchMa.containsMouse ? Tokens.ink : Tokens.line)
                                                Behavior on color { ColorAnimation { duration: Tokens.snap } }
                                                Behavior on border.color { ColorAnimation { duration: Tokens.snap } }
                                                IconText {
                                                    anchors.centerIn: parent
                                                    visible: !swatch.assigned
                                                    text: "palette"
                                                    color: swatchMa.containsMouse ? Tokens.inkMuted : Tokens.inkFaint
                                                    font.pixelSize: Tokens.fSmall
                                                }
                                                MouseArea {
                                                    id: swatchMa
                                                    anchors.fill: parent
                                                    hoverEnabled: true
                                                    cursorShape: Qt.PointingHandCursor
                                                    onClicked: {
                                                        if (page.colorGid === wr.gid) { page.colorGid = ""; page.colorLabel = "" }
                                                        else { page.colorGid = wr.gid; page.colorLabel = wr.modelData.label }
                                                    }
                                                }
                                            }
                                            UiText {
                                                anchors.verticalCenter: parent.verticalCenter
                                                text: swatch.assigned ? I18n.tr("Tinted. Tap to change.") : I18n.tr("Give this widget its own accent.")
                                                color: Tokens.inkFaint
                                                font.family: Tokens.ui
                                                font.pixelSize: Tokens.fSmall
                                            }
                                        }
                                    }

                                    // the widget's own catalogue settings.
                                    Repeater {
                                        model: wr.settings
                                        delegate: WSetting {
                                            required property var modelData
                                            wid: wr.wid
                                            setting: modelData
                                        }
                                    }

                                    // the workspaces widget keeps its live marker preview.
                                    CcWorkspacePreview {
                                        visible: wr.wid === "workspaces"
                                        width: parent.width
                                        root: page.root
                                        tk: page.tk
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    CcScrollRail { root: page.root; tk: page.tk; flick: flick; z: 5 }

    // ── per-widget colour popover, floated above the body ──
    Component {
        id: colorPopComp
        CcWidgetColorPopover {
            root: page.root
            tk: page.tk
            gid: page.colorGid
            label: page.colorLabel
            onDismissed: { page.colorGid = ""; page.colorLabel = "" }
        }
    }
    Loader {
        anchors.fill: parent
        z: 60
        active: page.colorSupported && page.colorGid !== ""
        sourceComponent: colorPopComp
    }
}
